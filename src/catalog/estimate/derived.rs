//! Summing the four terms into one figure, from the files an entry names.
//!
//! Split from the door above it when `estimate.rs` became `estimate/mod.rs`,
//! which may only declare. The terms, and why each errs high, are set out
//! there.

use std::fs;
use std::path::Path;

use crate::gguf::Metadata;

use super::super::entry::Entry;
use super::cache::{cache_bytes, sixteenths};
use super::served::{keeps_no_cache, predicts_tokens, runs_on_processor};
use super::shard::shard_of;

const MIB: u64 = 1024 * 1024;

/// The device context and compute buffers.
const OVERHEAD_BYTES: u64 = 1024 * MIB;

/// What the figure was worked out from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::catalog) enum Basis {
    /// The metadata the model file carries about itself.
    Metadata,
    /// The size of the files, because the metadata could not be read.
    Size,
}

/// A derived estimate, and what it rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::catalog) struct Derived {
    pub mib: u32,
    pub basis: Basis,
}

/// Derives the estimate for one entry from the files under `root`.
///
/// # Errors
///
/// Returns the reason when the model file itself cannot be sized, which is
/// the one input without which there is nothing to derive from. A draft or a
/// projector that is not there contributes nothing; the start will name it.
pub(in crate::catalog) fn derive(entry: &Entry, root: &Path) -> Result<Derived, String> {
    let model = entry.path.resolve(root);
    let metadata = Metadata::read(&model);
    let mut weights = weights_of(&model, metadata.as_ref().ok())?;
    let mut cache = 0u128;

    if let Some(draft) = &entry.draft_path {
        let draft = draft.resolve(root);
        weights += size_of(&draft).unwrap_or(0);
        // A multiple-token-prediction draft is a head on the model beside it,
        // not a model of its own, and it predicts from the cache that model
        // already keeps. It is given no second cache here because the server
        // allocates it none.
        //
        // This matters more than it looks. Such a sidecar carries the parent's
        // configuration in its own metadata -- the 1.3 GiB MTP head shipped
        // with Qwen3.8 27B declares the parent's sixty-five layers -- so
        // sizing a cache from it charges a full-sized model's cache to a file
        // a fiftieth its weight. Measured on this estate: the entry came out
        // at 76,298 MiB against 28,867 MiB actually resident, and 33,280 MiB
        // of that gap was a cache for a head that never had one.
        if !predicts_tokens(&entry.flags)
            && let Ok(metadata) = Metadata::read(&draft)
        {
            let keys = sixteenths(&entry.flags, ["ctkd", "cache-type-k-draft"]);
            let values = sixteenths(&entry.flags, ["ctvd", "cache-type-v-draft"]);
            cache += cache_bytes(&metadata, entry.context_size, keys, values).unwrap_or(0);
        }
    }
    if let Some(projector) = &entry.projector_path {
        weights += size_of(&projector.resolve(root)).unwrap_or(0);
    }

    let keys = sixteenths(&entry.flags, ["ctk", "cache-type-k"]);
    let values = sixteenths(&entry.flags, ["ctv", "cache-type-v"]);
    // Layers pinned to the processor are held in host memory, so every term
    // that scales with them -- the weights, the cache over them, and the
    // fragmentation between them -- is paid there rather than here. Zeroing
    // the weights collapses the fragmentation with them in either arm below,
    // and the overhead is left standing on its own, which is what such an
    // entry was measured to cost.
    let on_processor = runs_on_processor(&entry.flags);
    let cache = if on_processor { 0 } else { cache };
    let weights = if on_processor { 0 } else { u128::from(weights) };
    let (bytes, basis) = match metadata
        .ok()
        .and_then(|metadata| cache_bytes(&metadata, entry.context_size, keys, values))
    {
        // Zeroed rather than skipped: the metadata was read and the basis is
        // still what the file said, so this stays the metadata arm. Routing an
        // embedding entry to the arm below would charge it a quarter of its
        // weights as a cache proxy -- a smaller wrong answer, reported as
        // though nothing had been read.
        Some(model_cache) => (
            weights
                + if keeps_no_cache(&entry.flags) || on_processor {
                    0
                } else {
                    model_cache
                }
                + cache
                + weights * 5 / 100
                + u128::from(OVERHEAD_BYTES),
            Basis::Metadata,
        ),
        None => (
            weights + weights / 4 + u128::from(OVERHEAD_BYTES),
            Basis::Size,
        ),
    };
    let mib = u32::try_from(bytes.div_ceil(u128::from(MIB))).unwrap_or(u32::MAX);
    Ok(Derived { mib, basis })
}

/// The size of the model file, plus every other shard when it is split.
fn weights_of(model: &Path, metadata: Option<&Metadata>) -> Result<u64, String> {
    let mut total =
        size_of(model).ok_or_else(|| format!("no model file at '{}'", model.display()))?;
    let shards = metadata.and_then(Metadata::split_count).unwrap_or(1);
    if let Some(shard) = model
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(shard_of)
    {
        for index in 1..=shards {
            if index != shard.index {
                total += size_of(&shard.sibling(model, index)).unwrap_or(0);
            }
        }
    }
    Ok(total)
}

fn size_of(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
        .map(|metadata| metadata.len())
}
