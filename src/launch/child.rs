//! One running child, and whether it is still there.
//!
//! What a caller holds once [`Server::start`](super::Server::start) returns:
//! the types that outlive the call, beside the work in `server` that produces
//! them.

use std::net::SocketAddr;
use std::process::{self, ExitStatus};

use super::output::{self, LineSink};

/// Whether a child process still exists.
///
/// Distinct from readiness, which is whether it has finished loading. A child
/// is alive long before it is ready, sometimes by minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// The process is still there.
    Running,
    /// The process is gone, with the status it left behind.
    Exited(ExitStatus),
}

/// One running server process, serving one entry on one loopback port.
#[derive(Debug)]
pub struct Child {
    pub(super) id: String,
    pub(super) address: SocketAddr,
    pub(super) process: process::Child,
    /// What the process has written, passed on and the last of it kept.
    said: output::Said,
}

impl Child {
    /// A process just spawned for this entry, with its output drained into
    /// `sink`.
    ///
    /// Its standard output and error must be pipes: each is read on a thread
    /// of its own for as long as the process writes to it.
    pub(super) fn spawned(
        id: String,
        address: SocketAddr,
        mut process: process::Child,
        sink: &LineSink,
    ) -> Self {
        let said = output::Said::default();
        if let Some(stdout) = process.stdout.take() {
            said.drain(&id, stdout, sink);
        }
        if let Some(stderr) = process.stderr.take() {
            said.drain(&id, stderr, sink);
        }
        Self {
            id,
            address,
            process,
            said,
        }
    }

    /// What this child said last, as a clause a failure can end with, or
    /// nothing when it said nothing.
    pub(super) fn last_words(&self) -> String {
        let last = self.said.last();
        if last.is_empty() {
            return String::new();
        }
        format!("; it said last:\n  {}", last.join("\n  "))
    }

    /// Where this child answers.
    #[must_use]
    pub fn endpoint(&self) -> SocketAddr {
        self.address
    }

    /// The process identifier, which is how the machine is asked what this
    /// child holds once it has loaded.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Whether the process is still there.
    pub fn check(&mut self) -> Liveness {
        match self.process.try_wait() {
            Ok(Some(status)) => Liveness::Exited(status),
            // A status that cannot be read is reported as still running. The
            // readiness loop is bounded by the entry's budget rather than by
            // this answer, so guessing at death here would only turn an
            // unreadable status into a wrong one.
            Ok(None) | Err(_) => Liveness::Running,
        }
    }

    /// Stops the child and reaps it, so no zombie is left behind.
    ///
    /// Abrupt: `SIGKILL` on the Unix platforms and `TerminateProcess` on
    /// Windows. Asking politely first needs a platform dependency the standard
    /// library does not offer, and `llama-server` holds no durable state, so
    /// an abrupt stop loses only responses in flight -- of which there are
    /// none until something can make a request.
    ///
    /// Killing a child that has already exited fails, and that failure is
    /// dropped: it means the work this method exists to do is already done.
    pub fn stop(&mut self) {
        drop(self.process.kill());
        drop(self.process.wait());
    }
}

/// A child never outlives the value that represents it.
///
/// Without this, a caller that drops a `Child` on an error path leaves a
/// server holding a port with nothing left in the program that knows about
/// it. This is the ordinary case and it is avoidable; a hard kill of the
/// router is not, and stays in the risks.
impl Drop for Child {
    fn drop(&mut self) {
        self.stop();
    }
}
