//! A directory of a test's own, for the files it writes.

use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

/// How many scratch directories this process has made.
static MADE: AtomicU64 = AtomicU64::new(0);

/// A directory under the system temporary directory, removed when dropped.
///
/// The estate's path rule allows the temporary directory because it names a
/// platform rather than a machine. Every directory a test writes into comes
/// from here: `support::ModelsRoot` fills one with empty placeholders, the
/// search path with the stub's names, and these tests with model files that
/// have bytes in them.
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// Creates a directory no other live test holds.
    ///
    /// The process id keeps test processes apart, and the count keeps this
    /// process's directories apart. Neither a thread id nor the clock can
    /// stand in for them: nextest runs every test on a thread with the same
    /// id, and macOS reads the clock only to the microsecond, so two tests
    /// that start in the same microsecond would get the same name. The clock
    /// is here only to keep a new directory off one that an ended process
    /// with the same id left behind.
    ///
    /// # Panics
    ///
    /// If the directory is already there or cannot be made. A test that took
    /// over a directory someone else made would read that test's files.
    #[must_use]
    pub(crate) fn new(label: &str) -> Self {
        let unique = format!(
            "model-router-{label}-{}-{}-{}",
            process::id(),
            MADE.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("a clock after 1970")
                .as_nanos()
        );
        let root = env::temp_dir().join(unique);
        fs::create_dir(&root).expect("a temporary directory no other test holds");
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
