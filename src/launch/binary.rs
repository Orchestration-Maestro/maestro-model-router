//! Finding the server binary, and the named builds beside it.
//!
//! Separated from starting a child because finding a binary and running one
//! are different jobs, and the module-size gate on `server.rs` said so.
//!
//! The property that matters is that both callers look for exactly the same
//! file. Two spellings of "what a runtime is called" would eventually
//! disagree, and the failure would be a check that passed against a router
//! that could not start -- the worst of both, since the check is what buys the
//! confidence.

use std::path::{Path, PathBuf};

use super::failure::Failure;
use super::search::on_search_path;
use crate::catalog::Entry;

/// What the server is called. Located on the search path, never bundled.
const BINARY_NAME: &str = "llama-server";

/// What a named runtime is called on disk.
fn runtime_named(runtime: &str) -> String {
    format!("{BINARY_NAME}-{runtime}")
}

/// Where a named runtime resolves to, if anywhere.
fn runtime_binary(runtime: &str) -> Option<PathBuf> {
    on_search_path(&runtime_named(runtime))
}

/// The configured binary when there is one, otherwise the first
/// `llama-server` on the search path; [`Server::located`](super::Server::located)
/// says why each can fail.
pub(super) fn located(configured: Option<&Path>) -> Result<PathBuf, Failure> {
    if let Some(path) = configured {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(Failure::Unavailable(format!(
                "the configured server binary is not there: '{}'",
                path.display()
            )))
        };
    }

    on_search_path(BINARY_NAME).ok_or_else(|| {
        Failure::Unavailable(format!(
            "no server binary was configured, and no '{BINARY_NAME}' \
             was found on the search path"
        ))
    })
}

/// The binary this entry is served from.
///
/// The one the router was started with, unless the entry names a runtime.
/// A named one resolves on the search path as `llama-server-<name>`, which
/// is how an operator points at a second build without the catalog
/// carrying a path: the catalog says *which*, the machine says *where*.
///
/// Resolved per start rather than once, because one router serves entries
/// that need different builds and a single binary chosen at startup cannot
/// be right for both.
pub(super) fn for_entry(started_with: &Path, entry: &Entry) -> Result<PathBuf, Failure> {
    let Some(runtime) = entry.runtime.as_deref() else {
        return Ok(started_with.to_path_buf());
    };

    runtime_binary(runtime).ok_or_else(|| {
        Failure::Unavailable(format!(
            "entry '{}' needs the '{runtime}' runtime, and nothing named \
             '{}' is on the search path",
            entry.id,
            runtime_named(runtime)
        ))
    })
}
