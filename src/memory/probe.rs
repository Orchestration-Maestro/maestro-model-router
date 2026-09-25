//! Where the machine's own figures come from: a test's fixed numbers, or the
//! platform's tools, asked each time.

use std::path::PathBuf;

use super::command;
use super::figures::{DeviceMemory, Measurement};
use super::parse;

/// The figures a test states, in place of a machine.
#[derive(Debug, Clone, Default)]
pub struct Fixed {
    /// What the device query answers, or nothing when there is no device.
    pub device: Option<DeviceMemory>,
    /// What the system memory query answers.
    pub system_total_mib: Option<u64>,
    /// What every process measures as, whatever its pid.
    pub measurement: Measurement,
}

/// Where the machine's own figures come from.
#[derive(Debug)]
pub enum Probe {
    /// The numbers a test wants, answered without running anything.
    Fixed(Fixed),
    /// The machine this router runs on, asked each time.
    Machine(Machine),
}

/// The tools this machine turned out to have.
#[derive(Debug)]
pub struct Machine {
    /// `nvidia-smi`, when the machine has one; where, so it is found once.
    nvidia_smi: Option<PathBuf>,
}

impl Probe {
    /// A probe that knows nothing, which is every test's default: the budget
    /// then behaves exactly as it did before the machine could be asked.
    #[must_use]
    pub fn none() -> Self {
        Self::Fixed(Fixed::default())
    }

    /// Looks for the tools this machine has, once.
    #[must_use]
    pub fn detect() -> Self {
        Self::Machine(Machine {
            nvidia_smi: command::nvidia_smi(),
        })
    }

    /// Total and used device memory, or `None` when there is no device or
    /// nothing can report it.
    #[must_use]
    pub fn device(&self) -> Option<DeviceMemory> {
        match self {
            Self::Fixed(fixed) => fixed.device,
            Self::Machine(machine) => {
                let text = command::query(machine.nvidia_smi.as_deref()?, command::DEVICE_QUERY)?;
                parse::device(&text)
            }
        }
    }

    /// What the machine has in system memory, in mebibytes, or `None` when
    /// nothing can report it.
    #[must_use]
    pub fn system_total_mib(&self) -> Option<u64> {
        match self {
            Self::Fixed(fixed) => fixed.system_total_mib,
            Self::Machine(_) => command::system_total_mib(),
        }
    }

    /// What one running process holds, on each side it can be read on.
    #[must_use]
    pub fn measure(&self, pid: u32) -> Measurement {
        match self {
            Self::Fixed(fixed) => fixed.measurement,
            Self::Machine(machine) => Measurement {
                resident_mib: command::resident_mib(pid),
                device_mib: machine
                    .nvidia_smi
                    .as_deref()
                    .and_then(|tool| command::query(tool, command::PROCESS_QUERY))
                    .and_then(|text| parse::compute_apps(&text, pid)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process;

    use super::*;

    #[test]
    fn a_fixed_probe_answers_with_what_it_was_built_with_for_any_pid() {
        let probe = Probe::Fixed(Fixed {
            device: Some(DeviceMemory {
                total_mib: 4096,
                used_mib: 1024,
            }),
            system_total_mib: Some(16384),
            measurement: Measurement {
                resident_mib: Some(700),
                device_mib: Some(300),
            },
        });

        assert_eq!(probe.device().map(|device| device.free_mib()), Some(3072));
        assert_eq!(probe.system_total_mib(), Some(16384));
        assert_eq!(probe.measure(1).largest_mib(), Some(700));
        assert_eq!(probe.measure(99_999).largest_mib(), Some(700));
    }

    #[test]
    fn a_probe_that_knows_nothing_reports_nothing() {
        let probe = Probe::none();
        assert_eq!(probe.device(), None);
        assert_eq!(probe.system_total_mib(), None);
        assert_eq!(probe.measure(1), Measurement::UNKNOWN);
    }

    /// The real probe against this test's own process: the one measurement
    /// that can be taken on any machine the tests run on.
    ///
    /// On the Unix platforms `ps` is part of the base system, so a resident
    /// set is expected. Elsewhere the figure may legitimately be unknown --
    /// what is asserted everywhere is that asking never fails loudly.
    #[test]
    fn the_machine_probe_measures_this_process_or_says_it_cannot() {
        let probe = Probe::detect();
        let measured = probe.measure(process::id());

        if cfg!(unix) {
            assert!(
                measured.resident_mib.is_some_and(|mib| mib > 0),
                "a running process has a resident set, and ps reads it: {measured:?}"
            );
        }
        assert!(
            measured.resident_mib.is_none_or(|mib| mib > 0),
            "a resident set is positive or unknown, never zero: {measured:?}"
        );
    }
}
