//! Starting one entry's child, measuring what it holds, and saying both.
//!
//! Split from `mod.rs` along the seam a load leaves: admission decides that
//! a child may be started, and this is the starting -- the only part of
//! serving that takes seconds to minutes, and the only part whose outcome an
//! operator cannot see from any request. So it is said out loud: that a load
//! began and what it was expected to cost, and that it finished and what it
//! turned out to cost.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::catalog::Entry;
use crate::launch::{Failure, Server};
use crate::memory::Measurement;

use super::super::loaded::Loaded;
use super::Slots;

/// Writes one line for the operator.
///
/// Written rather than printed, with the error dropped: a closed standard
/// output is not a reason to stop serving, and `println!` panics on one --
/// which would end whichever thread said the line, and a reaper thread that
/// ends is idle unloading that silently stops.
pub(in crate::proxy) fn say(line: &str) {
    drop(writeln!(std::io::stdout(), "{line}"));
}

impl Slots {
    /// Starts a child for this entry, and measures it once it is ready.
    ///
    /// The measurement is taken the moment the child answers, which is a
    /// floor rather than a peak: the context fills as the model is used.
    /// That is why admission counts the larger of the estimate and this.
    ///
    /// # Errors
    ///
    /// Returns the [`Failure`] the launcher returned, having said so.
    pub(super) fn start(
        &self,
        entry: &Entry,
        server: &Server,
        root: &Path,
    ) -> Result<Loaded, Failure> {
        say(&format!(
            "{}: loading, estimated at {} MiB",
            entry.id, entry.memory_estimate_mib
        ));
        let free_before = self.budget.probe().device().map(|device| device.free_mib());
        let started = Instant::now();
        let child = match server.start(entry, root) {
            Ok(child) => child,
            Err(failure) => {
                say(&format!("{}: not loaded: {failure}", entry.id));
                return Err(failure);
            }
        };
        let measured = on_the_device(
            self.budget.probe().measure(child.pid()),
            free_before,
            self.budget.probe().device().map(|device| device.free_mib()),
        );
        say(&ready(entry, started.elapsed(), &measured));
        Ok(Loaded {
            child: Arc::new(child),
            last_used: Instant::now(),
            measured,
        })
    }
}

/// What a child holds on the device, including when the device cannot say.
///
/// A driver that reports nothing per process -- WSL's, on the machine this
/// router was written for -- left every model counted at its estimate. How
/// far the device's free memory fell while the child loaded says instead,
/// and is sound because loads are admitted one at a time: nothing else this
/// router starts moves the figure meanwhile. Anything else on the machine that
/// allocates at the same moment is counted too, and a model counted high is
/// the safe error. A figure the device reports per process is kept, being the
/// child's alone.
fn on_the_device(
    measured: Measurement,
    free_before: Option<u64>,
    free_after: Option<u64>,
) -> Measurement {
    let fell = free_before
        .zip(free_after)
        .and_then(|(before, after)| before.checked_sub(after))
        .filter(|fell| *fell > 0);
    Measurement {
        device_mib: measured.device_mib.or(fell),
        ..measured
    }
}

/// The line a finished load says: how long it took, what the machine saw it
/// holding on each side, and what the catalog had said -- so an estimate
/// that is wrong is visible the first time the model loads rather than the
/// first time something is refused because of it.
fn ready(entry: &Entry, took: Duration, measured: &Measurement) -> String {
    format!(
        "{}: ready in {:.1} s, measured {} resident and {} on the device \
         (catalog said {} MiB)",
        entry.id,
        took.as_secs_f64(),
        gibibytes(measured.resident_mib),
        gibibytes(measured.device_mib),
        entry.memory_estimate_mib
    )
}

/// A figure in gibibytes to one decimal place, or the admission that there
/// is none.
///
/// Integer arithmetic on purpose: a tenth of a gibibyte is the precision
/// an operator reads at, and going through a float to print it would only
/// add a cast to explain.
fn gibibytes(mib: Option<u64>) -> String {
    match mib {
        Some(mib) => {
            let tenths = mib * 10 / 1024;
            format!("{}.{} GiB", tenths / 10, tenths % 10)
        }
        None => "nothing readable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{RelativePath, Residency};
    use std::collections::BTreeMap;

    fn entry() -> Entry {
        Entry {
            id: "qwen3-06b".to_owned(),
            path: RelativePath::new("somewhere/model.gguf").expect("relative"),
            draft_path: None,
            projector_path: None,
            context_size: 4096,
            residency: Residency::OnDemand,
            memory_estimate_mib: 1024,
            reasoning_format: None,
            reasoning_effort: None,
            startup_timeout_seconds: 30,
            // The stock server: these fixtures are about other things.
            runtime: None,
            flags: BTreeMap::new(),
        }
    }

    #[test]
    fn the_ready_line_carries_both_measurements_and_what_the_catalog_said() {
        let line = ready(
            &entry(),
            Duration::from_millis(5_440),
            &Measurement {
                resident_mib: Some(4608),
                device_mib: Some(717),
            },
        );

        assert_eq!(
            line,
            "qwen3-06b: ready in 5.4 s, measured 4.5 GiB resident and 0.7 GiB \
             on the device (catalog said 1024 MiB)"
        );
    }

    /// What this machine's driver answers: a resident set, and nothing per
    /// process on the device.
    const UNREAD_ON_THE_DEVICE: Measurement = Measurement {
        resident_mib: Some(900),
        device_mib: None,
    };

    #[test]
    fn a_device_that_cannot_say_per_process_is_read_by_how_far_its_free_memory_fell() {
        assert_eq!(
            on_the_device(UNREAD_ON_THE_DEVICE, Some(31_000), Some(2_000)),
            Measurement {
                resident_mib: Some(900),
                device_mib: Some(29_000),
            },
            "a model that took 29000 MiB of the card is counted at that, not \
             at the 900 MiB its process shows resident"
        );
    }

    #[test]
    fn what_the_device_says_per_process_is_kept_over_what_its_free_memory_did() {
        let per_process = Measurement {
            resident_mib: Some(900),
            device_mib: Some(28_000),
        };
        assert_eq!(
            on_the_device(per_process, Some(31_000), Some(2_000)),
            per_process,
            "a figure that is the child's alone beats one that is the card's"
        );
    }

    #[test]
    fn free_memory_that_did_not_fall_or_could_not_be_read_says_nothing() {
        for (before, after) in [
            (Some(2_000), Some(2_000)),
            (Some(2_000), Some(3_000)),
            (None, Some(2_000)),
            (Some(2_000), None),
        ] {
            assert_eq!(
                on_the_device(UNREAD_ON_THE_DEVICE, before, after),
                UNREAD_ON_THE_DEVICE,
                "free memory of {before:?} then {after:?} says nothing about \
                 what the child took"
            );
        }
    }

    #[test]
    fn a_side_that_could_not_be_read_says_so_rather_than_saying_zero() {
        let line = ready(&entry(), Duration::from_secs(1), &Measurement::UNKNOWN);

        assert!(
            line.contains("nothing readable resident and nothing readable on the device"),
            "an unreadable figure is named as such, because a zero here would \
             tell the operator the model is free: {line}"
        );
    }
}
