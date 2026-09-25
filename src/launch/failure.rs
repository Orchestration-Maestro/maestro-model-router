//! Why a server could not be located, started, or resolved.
//!
//! Its own module because every part of launching raises it, and the proxy
//! branches on it; none of them should have to name the others to do so.

use std::error::Error;
use std::fmt;

/// Why a server could not be located, started, or resolved.
///
/// Slice 2 carried this as one opaque string, on the grounds that nothing
/// chose a branch on the kind of failure: it was printed and the command
/// exited. That was true of the only caller it had.
///
/// The proxy is the second caller and it does branch. A child that missed its
/// startup budget is a gateway timeout, and a child that could never start is
/// a bad gateway, so the difference has to survive the trip out of this
/// module. Three variants rather than one per cause: these are the three the
/// status mapping distinguishes, and a variant nothing reads would be the
/// speculative promise the original comment was right to refuse.
///
/// Matching on the message text was the alternative, and it is worse: it makes
/// the wording of an error a load-bearing interface that no test guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A child started, and did not answer inside the entry's startup budget.
    NotReady(String),
    /// A server could not be located, or a child could not be started at all.
    Unavailable(String),
    /// A child was not started, because there was no room for it.
    ///
    /// Distinct from [`Failure::Unavailable`] because nothing was attempted:
    /// the entry is serviceable and the machine is not broken, so a caller
    /// that waits for something else to finish may get a different answer.
    /// That difference is what the status codes carry.
    Refused(String),
    /// A child was not started, because the room it needed was taken by a
    /// request that reached it first.
    ///
    /// Distinct from [`Failure::Refused`] because the difference is what a
    /// caller should do next. A refusal names what holds the memory and may
    /// never change; this one changes the moment the other request is done,
    /// so it is the one answer a caller improves by retrying -- and the proxy
    /// says so with a `Retry-After`, which it could not do if the two shared
    /// a variant and differed only in prose.
    Contended(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::NotReady(message)
        | Self::Unavailable(message)
        | Self::Refused(message)
        | Self::Contended(message)) = self;
        write!(f, "{message}")
    }
}

impl Error for Failure {}
