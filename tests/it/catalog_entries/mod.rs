//! The model catalog: the shape of an entry, and what an entry is estimated
//! to cost.
//!
//! Split by what varies. `entry_schema` parses text and nothing else; the
//! estimates read model files from a scratch root, which is where every
//! derivation rule lives. `window_estimates` is apart from the rest of them
//! because it varies the model's layers rather than the entry.

mod entry_schema;
mod memory_estimates;
mod window_estimates;
