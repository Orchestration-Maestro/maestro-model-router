//! Where the router's own lines for its operator go.
//!
//! A load began and what it was expected to cost, a load finished and what it
//! turned out to cost, a model unloaded to make room or for sitting idle, a
//! child that exited on its own, a resident that would not load: the operator
//! has no request that would show any of these, so the router says them. A
//! library does not print, so the binary chooses where they go -- its own
//! standard output, and standard error for a resident that would not load --
//! and hands that choice in through `idle::Limits`.
//!
//! Apart from [`LineSink`](crate::launch::LineSink), which carries what a
//! *child* writes: these are the router's own words, not a child's.

use std::fmt;
use std::sync::Arc;

/// Where the router's lines for its operator are written: what it did, and a
/// resident that would not load.
///
/// Chosen by the binary rather than here, because a library does not print:
/// the router hands in one that writes to its own standard output and
/// standard error, and a caller that hands in none says nothing at all.
#[derive(Clone)]
pub struct Voice {
    /// What the router did, one line at a time.
    said: Arc<Line>,
    /// A resident that would not load, one line at a time.
    complained: Arc<Line>,
}

/// What a [`Voice`] calls with each line, without its line ending.
type Line = dyn Fn(&str) + Send + Sync;

impl Voice {
    /// A voice that passes what the router did to `said`, and a resident that
    /// would not load to `complained`.
    pub fn new(
        said: impl Fn(&str) + Send + Sync + 'static,
        complained: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            said: Arc::new(said),
            complained: Arc::new(complained),
        }
    }

    /// Says one line about what the router did.
    pub(crate) fn say(&self, line: &str) {
        (self.said)(line);
    }

    /// Says one line about a resident that would not load.
    pub(crate) fn complain(&self, line: &str) {
        (self.complained)(line);
    }
}

/// Says nothing.
impl Default for Voice {
    fn default() -> Self {
        Self::new(|_| {}, |_| {})
    }
}

impl fmt::Debug for Voice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Voice")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn each_line_reaches_the_side_it_was_said_on_and_only_that_side() {
        let (heard, lines) = mpsc::channel();
        let complaints = heard.clone();
        let voice = Voice::new(
            move |line| drop(heard.send(format!("said: {line}"))),
            move |line| drop(complaints.send(format!("complained: {line}"))),
        );

        voice.say("qwen38: loading, estimated at 100 MiB");
        voice.complain("resident qwen38: not found");
        drop(voice);

        assert_eq!(
            lines.iter().collect::<Vec<_>>(),
            [
                "said: qwen38: loading, estimated at 100 MiB",
                "complained: resident qwen38: not found",
            ]
        );
    }

    // Closures have no `Debug`, so the limits a voice travels in would print
    // nothing where it sits; it prints its name instead.
    #[test]
    fn a_voice_is_debugged_by_its_name() {
        assert_eq!(format!("{:?}", Voice::default()), "Voice");
    }
}
