//! The model catalog: the shape of an entry, and what an entry is estimated
//! to cost.
//!
//! Split by what varies. `entry_schema` parses text and nothing else; the
//! estimates read model files from a scratch root, which is where every
//! derivation rule lives.

mod entry_schema;
mod memory_estimates;
