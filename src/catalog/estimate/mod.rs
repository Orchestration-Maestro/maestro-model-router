//! What loading an entry is expected to cost, worked out from its files.
//!
//! An estimate decides what fits, and one written by hand is the figure most
//! likely to be wrong: the audit that led here found the shipped ones two to
//! four and a half times below what the same models measured once loaded,
//! because nothing derived the cache from the context size. This derives it.
//!
//! The figure is the sum of four terms, in bytes, rounded up to whole
//! mebibytes:
//!
//! - **weights**: the size of every file the entry names -- all the shards of
//!   a split model, the draft model, and the projector;
//! - **cache**: for the model, and for a draft that keeps one of its own,
//!   keys and values for every layer at the configured context:
//!   `layers x context x kv_heads x (key_length + value_length) x bytes`,
//!   where a missing key length is the embedding width over the head count,
//!   a missing value length is the key length, and bytes per element follows
//!   the cache-type flags (`ctk`/`ctv`, or their long spellings): f16 and
//!   bf16 hold 2, f32 holds 4, `q8_0` holds 1.0625, `q4_0` holds 0.5625, and
//!   any other spelling is read as f16. A draft used for multiple-token
//!   prediction (`spec-type` naming `mtp`) is given none: it is a head on the
//!   model beside it and predicts from that model's cache, while carrying the
//!   parent's layer count in its own metadata -- so sizing one charges a full
//!   model's cache to a file a fiftieth its weight;
//! - **fragmentation**: five percent of the weights, for the allocator's
//!   rounding and the padding between tensors;
//! - **overhead**: a fixed 1024 MiB for the device context and the compute
//!   buffers, which no file records and which a resident that holds no
//!   layers on the device was still measured to pay.
//!
//! An entry that pins every layer to the processor (`n-gpu-layers = 0`) is
//! charged the overhead and nothing else: the first three terms are all paid
//! in host memory, and the budget being spent here is the device's.
//!
//! A file whose metadata cannot be read is estimated from its size alone: a
//! quarter again on top, plus the overhead. It is rougher, and it is said.
//!
//! Every term errs high on purpose. An estimate above the cost leaves memory
//! idle; one below it admits a model the machine cannot hold, which is the
//! failure the budget exists to prevent.

mod cache;
mod derived;
mod served;
mod shard;

pub(super) use derived::{Basis, derive};
pub(super) use shard::shard_of;
