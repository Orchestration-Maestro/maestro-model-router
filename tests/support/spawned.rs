//! The router binary as a process, for the tests that need what only a
//! process has: a signal to receive, or a search path of its own.
//!
//! Moved here from `shutdown.rs` when a second target needed it. A test that
//! needs a child found under a chosen name gives this process that search
//! path through `Command::env`, rather than changing its own environment,
//! which is `unsafe` in Rust 2024 and changes it for every test in the binary.
//!
//! Unix only: the search path is built from symlinks, and the signal is sent
//! through `kill`.

use std::env::{self, consts};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{self, Child, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use super::{ModelsRoot, stub_binary};

/// A search path whose `llama-server` is the stub, so the binary under test
/// finds a child the way it does in the field: by name, on `PATH`.
pub struct SearchPath {
    directory: PathBuf,
}

impl SearchPath {
    /// A directory carrying the stub as `llama-server`.
    pub fn with_stub() -> Self {
        let directory = env::temp_dir().join(format!(
            "model-router-path-{}-{:?}",
            process::id(),
            thread::current().id()
        ));
        fs::create_dir_all(&directory).expect("a writable temporary directory");
        Self { directory }.linking("llama-server")
    }

    /// Also carries the stub as `llama-server-<runtime>`, the name a catalog
    /// entry's `runtime` resolves to.
    #[must_use]
    pub fn carrying(self, runtime: &str) -> Self {
        self.linking(&format!("llama-server-{runtime}"))
    }

    fn linking(self, name: &str) -> Self {
        // The suffix is derived even here, where it is always empty: the rule
        // is that no file name assumes a platform, and a rule with an
        // exception is a rule that gets copied without it.
        let file = format!("{name}{}", consts::EXE_SUFFIX);
        symlink(stub_binary(), self.directory.join(file))
            .expect("a symlink in the temporary directory");
        self
    }

    /// The current search path with this directory in front of it.
    pub fn value(&self) -> OsString {
        let mut paths = vec![self.directory.clone()];
        if let Some(inherited) = env::var_os("PATH") {
            paths.extend(env::split_paths(&inherited));
        }
        env::join_paths(paths).expect("a search path with no separator in it")
    }
}

impl Drop for SearchPath {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.directory));
    }
}

/// The router binary, serving, and ended when the test leaves however it
/// leaves.
pub struct RouterProcess {
    process: Child,
    stdout: BufReader<ChildStdout>,
}

impl RouterProcess {
    /// Starts `model-router serve` on an ephemeral port.
    pub fn serve(catalog: &Path, root: &ModelsRoot, search: &SearchPath) -> Self {
        Self::serve_with(catalog, root, search, &[])
    }

    /// Starts `model-router serve` on an ephemeral port, with these variables
    /// set in its environment as well.
    pub fn serve_with(
        catalog: &Path,
        root: &ModelsRoot,
        search: &SearchPath,
        variables: &[(&str, &str)],
    ) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_model-router"))
            .arg("serve")
            .arg(catalog)
            .arg("127.0.0.1:0")
            .env("PATH", search.value())
            .env("MAESTRO_MODELS_ROOT", root.path())
            // None is under test unless given below, and any inherited from
            // the shell would make the router do something this test did not
            // ask for.
            .env_remove("MAESTRO_MEMORY_BUDGET_MIB")
            .env_remove("MAESTRO_IDLE_UNLOAD_SECONDS")
            .env_remove("MAESTRO_API_KEY")
            .env_remove("MAESTRO_ALLOWED_ORIGINS")
            .envs(variables.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the router binary is built by cargo test");
        let stdout = BufReader::new(process.stdout.take().expect("a piped stdout"));
        Self { process, stdout }
    }

    /// The address the router says it is serving on, from its first line.
    pub fn address(&mut self) -> SocketAddr {
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("the router's first line");
        let address = line
            .trim()
            .strip_prefix("serving on http://")
            .unwrap_or_else(|| {
                panic!(
                    "the first line names the address; got {line:?}, and stderr said:\n{}",
                    self.stderr()
                )
            });
        address.parse().expect("an address after the scheme")
    }

    /// Sends the signal a service manager sends.
    ///
    /// Through the `kill` command, because `std`'s `Child::kill` sends
    /// `SIGKILL`, which no process can handle.
    pub fn terminate(&self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.process.id().to_string()])
            .status()
            .expect("the kill command");
        assert!(status.success(), "kill -TERM reached the router");
    }

    /// The exit status, if the router ends before the deadline.
    pub fn exited_within(&mut self, deadline: Duration) -> Option<ExitStatus> {
        let until = Instant::now() + deadline;
        while Instant::now() < until {
            if let Some(status) = self.process.try_wait().expect("the router's status") {
                return Some(status);
            }
            sleep(Duration::from_millis(25));
        }
        None
    }

    /// Whatever the router has written to stdout since the address line.
    ///
    /// Read only once the process has ended, so this cannot block on a pipe
    /// that is still open.
    pub fn rest_of_stdout(&mut self) -> String {
        let mut text = String::new();
        drop(self.stdout.read_to_string(&mut text));
        text
    }

    /// Whatever the router wrote to stderr, once it has ended.
    pub fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut stderr) = self.process.stderr.take() {
            drop(stderr.read_to_string(&mut text));
        }
        text
    }
}

impl Drop for RouterProcess {
    fn drop(&mut self) {
        // Signalled rather than killed, so a router that can stop its
        // children on a signal does so here too, whichever way the test ended.
        // Only a router that ignores the signal is then killed outright.
        if self.process.try_wait().ok().flatten().is_none() {
            self.terminate();
            if self.exited_within(Duration::from_secs(5)).is_none() {
                drop(self.process.kill());
            }
        }
        drop(self.process.wait());
    }
}
