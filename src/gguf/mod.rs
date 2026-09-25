//! What a model file says about itself.
//!
//! A GGUF file begins with its metadata: a magic, a version, two counts, then
//! typed key-value pairs, and only after all of that the tensors. Everything
//! the router wants to know about a model before loading it -- how many
//! layers it has, how wide its attention is, how much context it was trained
//! for, whether it is one shard of several -- sits in those pairs, so this
//! reads them and stops.
//!
//! Three things are part of the contract rather than the implementation.
//!
//! Nothing here allocates on the file's say-so. A length is stepped over, not
//! read into memory, unless the value is one of the handful this keeps; so a
//! file claiming a terabyte-long string costs a seek past the end of the file
//! and a fault, never that much memory. The router reads files it did not
//! write, and a corrupt download must not take it down.
//!
//! A per-layer setting reads as its largest value. Some architectures vary
//! the key-value head count by layer and store an array; the cache is sized
//! for the worst layer, so the largest is the honest figure and an average
//! would undercount.
//!
//! A key the file does not carry is absent, never zero. Zero layers or zero
//! heads would make an estimate of nothing, which is the one figure a caller
//! must never be handed by mistake.
//!
//! Versions 2 and 3 are read; they lay their metadata out identically. Version
//! 1 used narrower lengths and predates every file the router will meet.

mod bytes;
mod fault;
mod metadata;

pub use fault::Fault;
pub use metadata::Metadata;
