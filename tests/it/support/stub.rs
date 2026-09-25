//! The stub server's binary, which cargo builds beside the router's.

use std::path::PathBuf;

/// The stub server, which stands in for `llama-server` wherever a test needs
/// a process that answers the health contract without a model behind it.
#[must_use]
pub(crate) fn stub_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stub-llama-server"))
}
