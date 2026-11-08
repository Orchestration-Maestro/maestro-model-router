//! The repository's Rust files, and the root of each build target.

use std::fs;
use std::path::{Path, PathBuf};

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
    read_all(&repo_root(), sources())
}

/// Every Rust file of `paths`, keyed by its path from `root`. A file that
/// cannot be read is named in the failure rather than left out: dropped, it
/// would be a file both checks and this one report as covered while reading
/// nothing of it.
fn read_all(root: &Path, paths: Vec<PathBuf>) -> Sources {
    paths
        .into_iter()
        .filter(|path| has_extension(path, &["rs"]))
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading {} failed: {error}", path.display()));
            let relative = path
                .strip_prefix(root)
                .unwrap_or_else(|_| panic!("{} is not below {}", path.display(), root.display()));
            let parts: Vec<_> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            (parts.join("/"), text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::fixtures::Scratch;

    use super::read_all;

    #[test]
    #[should_panic(expected = "unreadable.rs")]
    fn a_file_that_cannot_be_read_is_named_instead_of_dropped() {
        let scratch = Scratch::new("rust-source-unreadable");
        let file = scratch.path().join("unreadable.rs");
        fs::write(&file, [0x66, 0xff, 0x6e]).expect("a writable file");
        drop(read_all(scratch.path(), vec![file]));
    }
}
