//! What an entry actually costs, and how fast it actually runs.
//!
//! A catalog's `memory_estimate_mib` is a claim. Until an entry has been
//! loaded on a machine, that claim is arithmetic over file sizes and a guess
//! at the context cache, and the two ways a guess goes wrong are not
//! symmetric: too low and the card runs out part-way through loading, too high
//! and the entry is refused while actually fitting. Neither failure says which
//! it was.
//!
//! This loads one entry, asks the card what it took, asks the server how fast
//! it generated, and stops it again. One at a time and never two: the number
//! wanted is what a *single* entry costs, and co-residency would attribute one
//! model's pages to another.
//!
//! It reports. It does not rewrite the catalog -- see the plan for why, but
//! briefly: the shipped catalog's comments carry the reasoning for the numbers
//! beside them, and a writer that preserved them is a TOML round-tripper.

mod measure;
mod rate;
mod report;

pub use measure::{Measurement, entry};
pub use rate::Throughput;
pub use report::command;
