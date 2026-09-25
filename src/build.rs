//! Which source a binary was built from.
//!
//! A router installed from a working tree nobody committed cannot be rebuilt,
//! compared or rolled back, and nothing in the running process said what it
//! was. The release comes from the manifest; the commit comes from the build,
//! which `just deploy` tells through `MAESTRO_MODEL_ROUTER_COMMIT`. A build that was
//! not told says so rather than guessing.

/// The release, from the manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit the build was told it came from, or `unrecorded`.
pub const COMMIT: &str = match option_env!("MAESTRO_MODEL_ROUTER_COMMIT") {
    Some(commit) => commit,
    None => "unrecorded",
};

/// The release and the commit, as `model-router --version` prints them.
#[must_use]
pub fn described() -> String {
    format!("model-router {VERSION} ({COMMIT})")
}
