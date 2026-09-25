//! One model the router can serve, and whether it is held loaded.
//!
//! Split from the door above it when `catalog.rs` became `catalog/mod.rs`,
//! which may only declare. What an entry can be asked for is in
//! `capability.rs`; how one is read from text is in `read.rs`.

use std::collections::BTreeMap;

use super::path::RelativePath;

/// Whether a model is held loaded or loaded when something asks for it.
///
/// A resident model is never evicted, which is what lets a small model answer
/// immediately while larger ones come and go around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residency {
    /// Loaded at startup and never evicted.
    Resident,
    /// Loaded on first use, and evictable afterwards.
    OnDemand,
}

impl Residency {
    /// The spelling used in a catalog, and the only two accepted.
    pub(crate) const NAMES: [&'static str; 2] = ["resident", "on-demand"];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "resident" => Some(Self::Resident),
            "on-demand" => Some(Self::OnDemand),
            _ => None,
        }
    }
}

/// One model the router can serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Names the model, never the role it happens to serve.
    pub id: String,
    /// The model weights.
    pub path: RelativePath,
    /// The smaller model used for speculative decoding, when there is one.
    pub draft_path: Option<RelativePath>,
    /// The multimodal projector, when the model takes more than text.
    pub projector_path: Option<RelativePath>,
    /// Tokens of context the server is started with.
    pub context_size: u32,
    /// Whether this model is held loaded.
    pub residency: Residency,
    /// What loading this model is expected to cost, in mebibytes.
    pub memory_estimate_mib: u32,
    /// How reasoning output is delimited, when the model produces any.
    pub reasoning_format: Option<String>,
    /// How much reasoning effort to ask for, when the model accepts a level.
    pub reasoning_effort: Option<String>,
    /// How long this model may take to become ready before the router gives up
    /// on it and says so.
    ///
    /// Per entry rather than global because startup time varies by two orders
    /// of magnitude: a small model answers in under a second, a large one on a
    /// cold page cache takes minutes. One value would be either too tight for
    /// the large entries or meaningless for the small ones.
    pub startup_timeout_seconds: u32,
    /// Which build of the server this entry needs, when it needs a particular
    /// one.
    ///
    /// A name, never a path: the catalog describes a set of models without
    /// naming the machine they sit on, and a path to a binary is the most
    /// machine-specific thing there is. The name selects `llama-server-<name>`
    /// on the search path, so an operator points it at their build the way
    /// they point at everything else -- by putting it where the router looks.
    ///
    /// `None` uses the server the router was started with. An entry needing a
    /// patched build -- speculative decoding against a sidecar the stock
    /// server cannot load -- names it, and the rest never think about it.
    pub runtime: Option<String>,
    /// Server settings this router passes through without interpreting.
    pub flags: BTreeMap<String, String>,
}
