//! What a router is given to serve: a models root and a catalog naming it.

use std::fs;
use std::path::Path;

use crate::fixtures::Scratch;

/// A models root with placeholder files in it, removed when the test ends.
///
/// A scratch directory of the test's own, so no other test's model is in it.
/// The files are empty: nothing in this slice reads a model, it only checks
/// that one is there.
pub(crate) struct ModelsRoot {
    root: Scratch,
}

impl ModelsRoot {
    /// Creates a root carrying each of the given relative locations.
    ///
    /// # Panics
    ///
    /// If the temporary directory cannot be written, which is a broken
    /// machine rather than a failing test.
    #[must_use]
    pub(crate) fn with(files: &[&str]) -> Self {
        let root = Scratch::new("models");
        for file in files {
            let path = root.path().join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("a writable temporary directory");
            }
            fs::write(&path, b"").expect("a writable placeholder");
        }
        Self { root }
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.root.path()
    }
}

/// The one model file every entry in these tests points at.
pub(crate) const MODEL: &str = "cache/gemma/gemma-3-1b.gguf";

/// A catalog carrying one entry called `gemma3`, plus whatever the test adds.
///
/// Written as text rather than built as a value, because that is how a
/// catalog reaches the router in the field, and a test that skipped the parser
/// would be agreeing with itself about the shape.
#[must_use]
pub(crate) fn catalog_text(extra: &str) -> String {
    format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         context_size = 4096\n\
         residency = \"on-demand\"\n\
         memory_estimate_mib = 512\n\
         startup_timeout_seconds = 30\n\
         \n\
         [models.gemma3]\n\
         path = \"{MODEL}\"\n\
         {extra}"
    )
}
