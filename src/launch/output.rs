//! What a child says, passed on and the last of it kept.
//!
//! A child's output used to go to the null device. A pipe nobody drains
//! blocks the child once it fills, and an inherited stream ties an orphan to
//! whatever its parent was writing to, so discarding it was the safe choice --
//! and it left a model that looped, or a server that died loading, with no
//! record anywhere of what it had said.
//!
//! Each stream is drained on a thread of its own instead. Every line goes on
//! to the router's own standard error, prefixed with the entry it came from,
//! so a service manager's journal keeps it; and the last few are kept, so a
//! child that dies while loading can be asked what it said on the way out.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// How many of a child's last lines are kept for a failure to quote.
const KEPT: usize = 5;

/// The longest line passed on whole. A child that writes without a newline --
/// a progress bar redrawn with carriage returns -- is split here rather than
/// held in memory until it stops.
const LONGEST_LINE: u64 = 4096;

/// How long a dead child's streams are given to reach their end, so its last
/// words are read before they are quoted.
const SETTLE: Duration = Duration::from_millis(500);

/// A child's output, as the threads draining it keep it.
#[derive(Debug, Default)]
struct Kept {
    lines: Mutex<VecDeque<String>>,
    /// Streams still being drained. Zero once both have reached their end.
    open: AtomicUsize,
}

/// The last lines a child wrote, shared with the threads that drain it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Said(Arc<Kept>);

impl Said {
    /// Drains one of a child's output streams on a thread of its own.
    pub(crate) fn drain(&self, id: &str, stream: impl Read + Send + 'static) {
        let said = self.clone();
        let id = id.to_owned();
        said.0.open.fetch_add(1, Ordering::SeqCst);
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
                    Ok(_) => said.passed_on(&id, &String::from_utf8_lossy(&line)),
                }
            }
            said.0.open.fetch_sub(1, Ordering::SeqCst);
        });
    }

    fn passed_on(&self, id: &str, line: &str) {
        let line = line.trim_end_matches(['\r', '\n']);
        eprintln!("{id}: {line}");
        let mut lines = self.0.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == KEPT {
            lines.pop_front();
        }
        lines.push_back(line.to_owned());
    }

    /// What the child said last, oldest first, once its streams have ended
    /// or [`SETTLE`] has passed, whichever comes first.
    pub(crate) fn last(&self) -> Vec<String> {
        let deadline = Instant::now() + SETTLE;
        while self.0.open.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let lines = self.0.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_lines_are_kept_and_the_oldest_are_let_go() {
        let said = Said::default();
        let output = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n";
        said.drain("model", std::io::Cursor::new(output.as_bytes().to_vec()));

        assert_eq!(
            said.last(),
            ["line 3", "line 4", "line 5", "line 6", "line 7"],
            "the five a failure quotes, oldest first"
        );
    }

    #[test]
    fn a_line_without_an_end_is_split_rather_than_held() {
        let said = Said::default();
        let long = "x".repeat(5000);
        said.drain("model", std::io::Cursor::new(long.into_bytes()));

        let last = said.last();
        assert_eq!(last.len(), 2, "split at the longest line: {last:?}");
        assert_eq!(last[0].len(), 4096);
        assert_eq!(last[1].len(), 904);
    }

    #[test]
    fn output_that_is_not_text_is_kept_readable() {
        let said = Said::default();
        said.drain("model", std::io::Cursor::new(vec![b'o', b'k', 0xff, b'\n']));
        assert_eq!(said.last(), ["ok\u{fffd}"]);
    }
}
