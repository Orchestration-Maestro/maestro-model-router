//! Waiting on a condition rather than on time.
//!
//! A test that sleeps a fixed time and then looks is either flaky on a loaded
//! machine or slow on an idle one. These ask the condition itself, again and
//! again, until it holds or a deadline passes. The deadline is a hang guard
//! rather than an assertion: nothing here times the router.

use std::thread;
use std::time::{Duration, Instant};

/// Asks `probe` until it answers, or `within` has passed; `None` when it
/// never did.
///
/// The thread parks for `every` between two asks, so a poll that waits
/// seconds does not spin a core the processes under test need. A park can end
/// early, which costs one ask more and nothing else: the answer, not the time
/// that passed, is what ends the wait.
pub(crate) fn polled<T>(
    within: Duration,
    every: Duration,
    mut probe: impl FnMut() -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(answer) = probe() {
            return Some(answer);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::park_timeout(every);
    }
}

/// Whether `condition` came to hold within `within`, asked every `every`.
pub(crate) fn eventually(
    within: Duration,
    every: Duration,
    mut condition: impl FnMut() -> bool,
) -> bool {
    polled(within, every, || condition().then_some(())).is_some()
}
