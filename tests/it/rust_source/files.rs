//! The repository's Rust files, and the root of each build target.

use std::fs;

use crate::common::{has_extension, repo_root, sources};
use crate::rust_source::tree::Sources;

/// The root file of every build target: the library, the two binaries and the
/// one integration-test crate. A target added to `Cargo.toml` is added here,
/// and `source_coverage` fails until it is.
pub(crate) const ROOTS: &[&str] = &[
    "src/lib.rs",
    "src/main.rs",
    "src/bin/stub_llama_server/main.rs",
    "tests/it/main.rs",
];

/// Every Rust file under the repository, keyed by its path from the root.
pub(crate) fn repository() -> Sources {
    let root = repo_root();
    sources()
        .into_iter()
        .filter(|path| has_extension(path, &["rs"]))
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let relative = path.strip_prefix(&root).ok()?;
            let parts: Vec<_> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            Some((parts.join("/"), text))
        })
        .collect()
}
