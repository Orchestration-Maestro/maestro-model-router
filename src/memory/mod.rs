//! What the machine says about its memory, asked at run time.
//!
//! The budget is a ceiling on estimates, and an estimate is what somebody
//! typed into a catalog. This module is the other source of truth: what the
//! device reports as free before a model is started, and what a child turns
//! out to hold once it has loaded. Neither replaces the catalog -- a figure
//! nobody can read stays an estimate -- but where the machine can be asked,
//! it is asked, and the answer is trusted over the guess.
//!
//! Everything here is fallible and everything degrades the same way: a tool
//! that is missing, hangs, or prints something unreadable makes the figure
//! *unknown*, never zero and never a panic. An unknown figure is what the
//! router already lived with before this module existed.
//!
//! The probe is a value rather than a set of free functions so a test can
//! state the numbers it means. [`Probe::Fixed`] answers with what it was
//! built with; [`Probe::Machine`] runs the platform's tools. The router only
//! ever holds one of these, and nothing that acts on a figure knows which.

mod command;
mod figures;
mod parse;
mod probe;

pub use figures::{DeviceMemory, Measurement};
pub use probe::{Fixed, Machine, Probe};
