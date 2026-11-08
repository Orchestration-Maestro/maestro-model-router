//! The repository's files, walked once for every gate that reads them.

use std::fs;
use std::path::{Path, PathBuf};

/// The repository root. One crate at the top level, so the manifest directory
/// is the root, and the rules apply to every file below it.
pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file in the repository, minus build output and tool state. Callers
/// filter by extension: one walk serves the prose, link and size gates.
pub(crate) fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        // `reports` holds local evidence the .gitignore keeps out of the
        // repository; CI never sees it, so neither do the gates here.
        let skip = [
            "target",
            ".git",
            "node_modules",
            ".worktrees",
            ".superpowers",
            "reports",
        ];
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if skip.contains(&name.as_str()) {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(&repo_root(), &mut out);
    out
}

/// True when the path carries one of the given extensions.
pub(crate) fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extensions.contains(&extension))
}

/// The index of the first line that opens test code in a source file, if any.
///
/// Any cfg naming `test` counts, not just the bare `#[cfg(test)]`. Matching the
/// literal string would charge a module for a `#[cfg(all(test, unix))]` block --
/// the gate doing the exact thing the size gate says it must not.
pub(crate) fn test_start(text: &str) -> Option<usize> {
    text.lines().position(|line| {
        let line = line.trim_start();
        line.starts_with("#[cfg(")
            && line
                .split(|letter: char| !letter.is_alphanumeric() && letter != '_')
                .any(|tok| tok == "test")
    })
}
