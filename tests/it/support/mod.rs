//! Shared by the tests that drive real processes.
//!
//! Separate from `common`, which walks the repository for the prose and size
//! gates. These two sets of helpers have nothing to do with each other, and a
//! single module carrying both would be a module named after where it sits
//! rather than what it does.
//!
//! Each test module uses a subset: supervision drives children directly and
//! never sends a request, the proxy tests send requests and never look at a
//! child. The helpers are named through this door; `poll` and `spawned` are
//! named through their own modules, which say what they are.

mod http;
mod models;
pub(crate) mod poll;
mod router;
#[cfg(unix)]
pub(crate) mod spawned;
mod stub;

pub(crate) use http::{arrivals, get, health, post, request, status};
pub(crate) use models::{MODEL, ModelsRoot, catalog_text};
pub(crate) use router::{
    Serving, budgeted, capped, guarded, impatient, probed, queued, reloadable, serving, settled,
    windowed,
};
pub(crate) use stub::stub_binary;
