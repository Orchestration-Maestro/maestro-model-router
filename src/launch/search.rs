//! Finding a program on the search path, as the operating system would.
//!
//! Its own module because two callers walk the path: the launcher, for the
//! server binary and the named builds beside it, and `memory`, for the device
//! tool. A second copy of the walk would be the duplication the gate exists
//! to refuse.

use std::env;
use std::path::PathBuf;

/// The first match for a name on the search path, with the platform's
/// executable suffix, so the Windows leg finds `llama-server.exe`.
pub(crate) fn on_search_path(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", env::consts::EXE_SUFFIX);
    let search = env::var_os("PATH")?;
    env::split_paths(&search)
        .map(|directory| directory.join(&file))
        .find(|candidate| candidate.is_file())
}
