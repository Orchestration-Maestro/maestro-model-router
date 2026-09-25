//! What the router holds, in the text format Prometheus scrapes.
//!
//! The same questions `/models` answers for a llama.cpp client -- what is
//! loaded, what it was estimated at, what it was measured holding -- asked on
//! a schedule and kept, so a graph can show what was loaded when the machine
//! ran short. Gauges only: each is read from the state the router already
//! keeps for its own decisions, so a scrape costs a snapshot and can never
//! drift from what admission sees.
//!
//! Like every answer the router gives out of its own state, a scrape starts
//! nothing.

use std::collections::HashMap;
use std::io;
use std::iter;
use std::net::TcpStream;

use super::reply;
use super::shared::Shared;
use crate::build::{COMMIT, VERSION};

/// The exposition format's own content type, which a scraper checks.
const TEXT_FORMAT: &str = "text/plain; version=0.0.4";

/// Answers one scrape.
pub(super) fn answer(stream: &TcpStream, shared: &Shared, head_only: bool) -> io::Result<()> {
    let catalog = shared.catalog();
    let loaded = shared.slots.loaded(&catalog);
    let held: HashMap<String, Option<u64>> = shared.slots.memory(&catalog).into_iter().collect();
    let model = |id: &str| format!("model=\"{}\"", label(id));

    let mut out = String::new();
    gauge(
        &mut out,
        "model_router_build_info",
        "The release and commit answering, as labels.",
        [(
            format!(
                "version=\"{}\",commit=\"{}\"",
                label(VERSION),
                label(COMMIT)
            ),
            1,
        )],
    );
    gauge(
        &mut out,
        "model_router_model_loaded",
        "Whether a child is holding the entry.",
        catalog
            .entries
            .iter()
            .map(|entry| (model(&entry.id), u64::from(loaded.contains(&entry.id)))),
    );
    gauge(
        &mut out,
        "model_router_model_declared_mib",
        "What the catalog estimates the entry holds, in MiB.",
        catalog
            .entries
            .iter()
            .map(|entry| (model(&entry.id), u64::from(entry.memory_estimate_mib))),
    );
    // Only what was measured: an entry not loaded, or whose card could not be
    // read, holds no measurement, which is not the same as holding nothing.
    gauge(
        &mut out,
        "model_router_model_held_mib",
        "What a loaded entry was measured holding, in MiB.",
        catalog.entries.iter().filter_map(|entry| {
            let mib = held.get(&entry.id).copied().flatten()?;
            Some((model(&entry.id), mib))
        }),
    );
    gauge(
        &mut out,
        "model_router_requests_waiting",
        "Requests in line for room.",
        [(String::new(), shared.slots.waiting() as u64)],
    );
    // Left out rather than reported as zero when there is no budget: zero
    // would read as a ceiling nothing fits under.
    if let Some(budget) = shared.slots.budget_mib() {
        gauge(
            &mut out,
            "model_router_memory_budget_mib",
            "The ceiling models are unloaded to stay under, in MiB.",
            [(String::new(), u64::from(budget))],
        );
    }
    reply::text(stream, TEXT_FORMAT, &out, head_only)
}

/// Writes one gauge: what it means, that it is a gauge, and one sample per
/// label set -- none when the labels are empty.
fn gauge(
    out: &mut String,
    name: &str,
    help: &str,
    samples: impl IntoIterator<Item = (String, u64)>,
) {
    // Extended with formatted lines rather than written to: writing to a
    // String cannot fail, and `write!` would still hand back an error for
    // this to dismiss.
    let lines = samples.into_iter().map(|(labels, value)| {
        if labels.is_empty() {
            format!("{name} {value}\n")
        } else {
            format!("{name}{{{labels}}} {value}\n")
        }
    });
    out.extend(iter::once(format!("# HELP {name} {help}\n# TYPE {name} gauge\n")).chain(lines));
}

/// A label value as the format quotes it. A catalog key is TOML, which may
/// quote any character, so an entry's name can carry the three the format
/// escapes.
fn label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_escapes_what_would_end_or_break_it() {
        assert_eq!(label("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
    }

    #[test]
    fn a_label_with_nothing_to_escape_is_left_alone() {
        assert_eq!(label("gemma3-4b.q4"), "gemma3-4b.q4");
    }

    #[test]
    fn a_gauge_without_labels_is_written_bare() {
        let mut out = String::new();
        gauge(&mut out, "up", "Whether it is.", [(String::new(), 1)]);
        assert_eq!(out, "# HELP up Whether it is.\n# TYPE up gauge\nup 1\n");
    }
}
