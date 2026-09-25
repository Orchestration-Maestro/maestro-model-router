//! A scratch directory for a test that writes model files with bytes in them.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, thread};

/// A directory under the system temporary directory, removed when dropped.
///
/// The estate's path rule allows the temporary directory because it names a
/// platform rather than a machine. Distinct from `support::ModelsRoot`, which
/// creates empty placeholder files: these tests need files with bytes in them.
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    #[must_use]
    pub(crate) fn new(label: &str) -> Self {
        let unique = format!(
            "model-router-{label}-{}-{:?}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("a clock after 1970")
                .as_nanos(),
            thread::current().id()
        );
        let root = env::temp_dir().join(unique);
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}
