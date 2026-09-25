//! What a child says, passed on and the last of it kept.
//!
//! A child's output used to go to the null device. A pipe nobody drains
//! blocks the child once it fills, and an inherited stream ties an orphan to
//! whatever its parent was writing to, so discarding it was the safe choice --
//! and it left a model that looped, or a server that died loading, with no
//! record anywhere of what it had said.
//!
//! Each stream is drained on a thread of its own instead. Every line goes on
//! to the [`LineSink`] the binary chose, with the entry it came from -- the
//! router's own standard error, so a service manager's journal keeps it; and
//! the last few are kept, so a child that dies while loading can be asked what
//! it said on the way out.

use std::collections::VecDeque;
use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

/// How many of a child's last lines are kept for a failure to quote.
const KEPT: usize = 5;

/// The longest line passed on whole. A child that writes without a newline --
/// a progress bar redrawn with carriage returns -- is split here rather than
/// held in memory until it stops.
const LONGEST_LINE: u64 = 4096;

/// How long a dead child's streams are given to reach their end, so its last
/// words are read before they are quoted.
const SETTLE: Duration = Duration::from_millis(500);

/// Where the lines a child writes are passed on to: the entry's id and the
/// line, without its line ending.
///
/// Chosen by the binary rather than here, because a library does not print:
/// the router hands in one that writes to its own standard error, and a
/// caller that hands in none still has the last lines kept for a failure to
/// quote, with nothing passed on.
#[derive(Clone)]
pub struct LineSink(Arc<PassOn>);

/// What a [`LineSink`] calls with each line: the entry's id, then the line.
type PassOn = dyn Fn(&str, &str) + Send + Sync;

impl LineSink {
    /// A sink that passes each line to `pass_on`, with the entry it came from.
    pub fn new(pass_on: impl Fn(&str, &str) + Send + Sync + 'static) -> Self {
        Self(Arc::new(pass_on))
    }
}

/// Passes nothing on.
impl Default for LineSink {
    fn default() -> Self {
        Self::new(|_, _| {})
    }
}

impl fmt::Debug for LineSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LineSink")
    }
}

/// A child's output, as the threads draining it keep it.
#[derive(Debug, Default)]
struct Kept {
    lines: Mutex<VecDeque<String>>,
    /// Streams still being drained. Zero once both have reached their end.
    open: Mutex<usize>,
    /// Signalled each time a stream reaches its end.
    ended: Condvar,
}

/// The last lines a child wrote, shared with the threads that drain it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Said(Arc<Kept>);

impl Said {
    /// Drains one of a child's output streams on a thread of its own, passing
    /// each line on to `sink`.
    pub(crate) fn drain(&self, id: &str, stream: impl Read + Send + 'static, sink: &LineSink) {
        let said = self.clone();
        let sink = sink.clone();
        let id = id.to_owned();
        *said.0.open.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut line = Vec::new();
            loop {
                line.clear();
                match (&mut reader)
                    .take(LONGEST_LINE)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) => said.passed_on(&id, &String::from_utf8_lossy(&line), &sink),
                }
            }
            *said.0.open.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            said.0.ended.notify_all();
        });
    }

    fn passed_on(&self, id: &str, line: &str, sink: &LineSink) {
        let line = line.trim_end_matches(['\r', '\n']);
        (sink.0)(id, line);
        let mut lines = self.0.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == KEPT {
            lines.pop_front();
        }
        lines.push_back(line.to_owned());
    }

    /// What the child said last, oldest first, once its streams have ended
    /// or [`SETTLE`] has passed, whichever comes first.
    pub(crate) fn last(&self) -> Vec<String> {
        let open = self.0.open.lock().unwrap_or_else(PoisonError::into_inner);
        drop(
            self.0
                .ended
                .wait_timeout_while(open, SETTLE, |open| *open > 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
        let lines = self.0.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Write};
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    // Closures have no `Debug`, so the server a sink travels in would print
    // nothing where it sits; it prints its name instead.
    #[test]
    fn a_sink_is_debugged_by_its_name() {
        assert_eq!(format!("{:?}", LineSink::default()), "LineSink");
    }

    #[test]
    fn the_last_lines_are_kept_and_the_oldest_are_let_go() {
        let said = Said::default();
        let output = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n";
        said.drain(
            "model",
            Cursor::new(output.as_bytes().to_vec()),
            &LineSink::default(),
        );

        assert_eq!(
            said.last(),
            ["line 3", "line 4", "line 5", "line 6", "line 7"],
            "the five a failure quotes, oldest first"
        );
    }

    #[test]
    fn every_line_is_passed_on_with_the_entry_it_came_from_and_without_its_ending() {
        let said = Said::default();
        let (heard, hearing) = mpsc::channel();
        let sink = LineSink::new(move |id, line| drop(heard.send(format!("{id}: {line}"))));
        said.drain("model", Cursor::new(b"one\r\ntwo\n".to_vec()), &sink);

        assert_eq!(said.last(), ["one", "two"], "the stream has ended");
        let passed: Vec<String> = hearing.try_iter().collect();
        assert_eq!(passed, ["model: one", "model: two"]);
    }

    #[test]
    fn a_line_without_an_end_is_split_rather_than_held() {
        let said = Said::default();
        let long = "x".repeat(5000);
        said.drain(
            "model",
            Cursor::new(long.into_bytes()),
            &LineSink::default(),
        );

        let last = said.last();
        assert_eq!(last.len(), 2, "split at the longest line: {last:?}");
        assert_eq!(last[0].len(), 4096);
        assert_eq!(last[1].len(), 904);
    }

    #[test]
    fn output_that_is_not_text_is_kept_readable() {
        let said = Said::default();
        said.drain(
            "model",
            Cursor::new(vec![b'o', b'k', 0xff, b'\n']),
            &LineSink::default(),
        );
        assert_eq!(said.last(), ["ok\u{fffd}"]);
    }

    #[test]
    fn a_stream_still_open_is_waited_for_until_it_ends() {
        let said = Said::default();
        let (output, mut child) = io::pipe().expect("a pipe");
        said.drain("model", output, &LineSink::default());
        let (asking, asked) = mpsc::channel();
        let speaking = thread::spawn(move || {
            child
                .write_all(b"early\n")
                .expect("the first line, written");
            // The last line waits until its words are being asked for, so the
            // stream is still open when they are.
            asked.recv().expect("the words, about to be asked for");
            child.write_all(b"late\n").expect("the last line, written");
        });

        asking.send(()).expect("the child is still speaking");
        assert_eq!(
            said.last(),
            ["early", "late"],
            "the last words are the ones said before the stream ended"
        );
        speaking.join().expect("the child finished speaking");
    }

    #[test]
    fn once_every_stream_has_ended_its_lines_are_given_at_once() {
        let said = Said::default();
        said.drain(
            "model",
            Cursor::new(b"done\n".to_vec()),
            &LineSink::default(),
        );
        let started = Instant::now();

        assert_eq!(said.last(), ["done"]);
        assert!(
            started.elapsed() < SETTLE,
            "waited {:?} on streams that had ended",
            started.elapsed()
        );
    }

    #[test]
    fn a_stream_that_never_ends_is_given_up_on() {
        // What a grandchild does when it inherits the pipe and outlives the
        // child: the stream stays open with nobody left to end it.
        let said = Said::default();
        let (output, mut held) = io::pipe().expect("a pipe");
        held.write_all(b"still here\n").expect("a line, written");
        said.drain("model", output, &LineSink::default());
        let started = Instant::now();

        assert_eq!(said.last(), ["still here"]);
        let waited = started.elapsed();
        assert!(
            waited >= SETTLE && waited < SETTLE * 4,
            "gave up after {waited:?}, not once {SETTLE:?} had passed"
        );
        drop(held);
    }
}
