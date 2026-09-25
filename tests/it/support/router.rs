//! A router serving on a thread of its own, and the ways a test starts one.
//!
//! One family: every launcher here names the one setting its tests are about
//! and hands the rest to the shared launch at the bottom, so a test that is
//! not about eviction, idle unloading or queueing says nothing about them.

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use maestro_model_router::admission::Budget;
use maestro_model_router::catalog::Catalog;
use maestro_model_router::idle::{IdleWindow, Limits};
use maestro_model_router::launch::Server;
use maestro_model_router::memory::Probe;
use maestro_model_router::proxy::{Access, Router, Source};
use maestro_model_router::queue::Wait;

use super::models::ModelsRoot;
use super::poll::eventually;
use super::stub::stub_binary;

/// A router bound to an ephemeral port, serving on a thread of its own.
///
/// The models root is held here so it outlives the router: dropping it would
/// remove the files the router resolves entries against while it is still
/// serving them.
pub(crate) struct Serving {
    address: SocketAddr,
    router: Arc<Router>,
    _root: ModelsRoot,
    /// The catalog file this router reads, for the tests that rewrite it.
    ///
    /// `None` for every helper but `reloadable`: the rest hand the router a
    /// catalog parsed from a string and never touch a file, so offering them
    /// a `rewrite` that appeared to work would be a trap.
    source: Option<PathBuf>,
}

/// Ends the router's children when the test that started them ends.
///
/// Without this every test leaves a server process behind: `serve` never
/// returns, so the router is never dropped, so the children it holds are never
/// stopped -- and a child is a separate process that outlives the one that
/// started it.
impl Drop for Serving {
    fn drop(&mut self) {
        self.router.stop();
    }
}

impl Serving {
    /// Where the router is listening.
    #[must_use]
    pub(crate) fn address(&self) -> SocketAddr {
        self.address
    }

    /// Which entries hold a child, without asking any of them for anything.
    #[must_use]
    pub(crate) fn loaded(&self) -> Vec<String> {
        self.router.loaded()
    }

    /// How many requests are waiting in line for room.
    #[must_use]
    pub(crate) fn waiting(&self) -> usize {
        self.router.waiting()
    }

    /// Residents the startup loader could not load.
    #[must_use]
    pub(crate) fn resident_failures(&self) -> Vec<String> {
        self.router.resident_failures()
    }

    /// Records which file this router reads its catalog from.
    #[must_use]
    fn reading(mut self, source: PathBuf) -> Self {
        self.source = Some(source);
        self
    }

    /// Replaces the catalog file, without telling the router.
    ///
    /// Writing is all this does. Making the router notice is `POST /reload`,
    /// and keeping the two apart is the point: a test that wants to show an
    /// edit alone changes nothing needs to be able to make one.
    ///
    /// # Panics
    ///
    /// If this router was not built by `reloadable`, which has no file to
    /// rewrite, or if the file cannot be written.
    pub(crate) fn rewrite(&self, catalog: &str) {
        let source = self
            .source
            .as_ref()
            .expect("a router built by `reloadable`, which is the one with a file");
        fs::write(source, catalog).expect("a writable temporary directory");
    }
}

/// Waits until the router satisfies a condition, or fails saying what it saw.
///
/// Residents load on a thread of their own, so a test that asserted the moment
/// it started serving would be racing the loader rather than testing it.
///
/// Nothing here asserts a duration. The deadline is a hang guard, generous
/// because a loaded continuous-integration machine is slow and because Windows
/// spawns a child roughly three times slower than Linux; a test that turned
/// that difference into an assertion would fail on the platform rather than on
/// the behaviour.
///
/// # Panics
///
/// If the condition has not arrived by the deadline, reporting what was loaded
/// and what failed so the failure names a state rather than only a timeout.
pub(crate) fn settled(serving: &Serving, expected: &str, done: impl Fn(&Serving) -> bool) {
    let arrived = eventually(Duration::from_secs(20), Duration::from_millis(20), || {
        done(serving)
    });
    assert!(
        arrived,
        "the router never {expected}; loaded {:?}, resident failures {:?}",
        serving.loaded(),
        serving.resident_failures()
    );
}

/// Binds a router on an ephemeral port and starts serving it.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn serving(catalog: &str, root: ModelsRoot) -> Serving {
    budgeted(catalog, root, None)
}

/// The same, under a stated memory budget.
///
/// Separate from `serving` so the tests that are not about eviction say
/// nothing about it, and so the ones that are state their ceiling at the call
/// rather than through the environment -- which is process-global and would
/// race every other test in the binary.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn budgeted(catalog: &str, root: ModelsRoot, limit_mib: Option<u32>) -> Serving {
    windowed(catalog, root, limit_mib, Duration::ZERO)
}

/// The same, under a stated memory budget and a stated idle window.
///
/// A window is injected directly rather than through
/// `MAESTRO_IDLE_UNLOAD_SECONDS`, for the same reason `budgeted` injects a
/// ceiling instead of setting `MAESTRO_MEMORY_BUDGET_MIB`: that variable is
/// process-global and would race every other test in this binary. Zero means
/// off, exactly as `IdleWindow::new` treats it.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn windowed(
    catalog: &str,
    root: ModelsRoot,
    limit_mib: Option<u32>,
    idle_window: Duration,
) -> Serving {
    launched(
        catalog,
        root,
        Budget::new(limit_mib),
        idle_window,
        Duration::ZERO,
    )
}

/// The same, under a stated budget and a stated wait for room.
///
/// Zero, which every other helper here passes, is the behaviour this router
/// had before waiting existed: a request whose room is held is refused at
/// once. Only the tests that are about queueing state anything else, so the
/// rest keep asserting what they were written against.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn queued(
    catalog: &str,
    root: ModelsRoot,
    limit_mib: Option<u32>,
    wait: Duration,
) -> Serving {
    launched(catalog, root, Budget::new(limit_mib), Duration::ZERO, wait)
}

/// The same, under a stated memory budget, on a machine whose figures the
/// test states.
///
/// What the device reports free and what every child measures as are
/// injected rather than read, for the reason every other setting here is:
/// a test that read the machine would assert about whichever machine it
/// happened to run on.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn probed(
    catalog: &str,
    root: ModelsRoot,
    limit_mib: Option<u32>,
    probe: Probe,
) -> Serving {
    launched(
        catalog,
        root,
        Budget::with_probe(limit_mib, probe),
        Duration::ZERO,
        Duration::ZERO,
    )
}

/// A router serving a catalog written to a file, which a test can rewrite.
///
/// Every other helper here hands the router a catalog parsed from a string,
/// because every other test asks what a fixed catalog does. Reload is the one
/// question that needs the file itself: the router re-reads its own source,
/// so a test that never wrote one has nothing to change.
///
/// The file goes in the models root, which already exists for the duration of
/// the test and is already removed with it.
///
/// # Panics
///
/// If the catalog cannot be written or is not usable, or the port cannot be
/// bound, which is a broken test rather than a failing one.
#[must_use]
pub(crate) fn reloadable(catalog: &str, root: ModelsRoot) -> Serving {
    let source = root.path().join("catalog.toml");
    fs::write(&source, catalog).expect("a writable temporary directory");
    launched(
        catalog,
        root,
        Budget::new(None),
        Duration::ZERO,
        Duration::ZERO,
    )
    .reading(source)
}

/// Binds a router with a budget already built, and starts serving it.
fn launched(
    catalog: &str,
    root: ModelsRoot,
    budget: Budget,
    idle_window: Duration,
    wait: Duration,
) -> Serving {
    let limits = Limits::new(budget, IdleWindow::new(idle_window), Wait::new(wait));
    launched_with(catalog, root, limits)
}

/// A router under a stated memory budget that gives up on a caller making no
/// progress after `stall`, rather than after the minute a router in service
/// gives -- which a test has no time to spend.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn impatient(
    catalog: &str,
    root: ModelsRoot,
    limit_mib: Option<u32>,
    stall: Duration,
) -> Serving {
    launched_with(catalog, root, open_limits(limit_mib).with_stall(stall))
}

/// A router that answers at most `connections` callers at once, with no
/// budget, no idle window and no wait.
///
/// # Panics
///
/// If the catalog is not usable or the port cannot be bound, which is a broken
/// test rather than a failing one.
#[must_use]
pub(crate) fn capped(catalog: &str, root: ModelsRoot, connections: usize) -> Serving {
    launched_with(
        catalog,
        root,
        open_limits(None).with_connections(connections),
    )
}

/// A router serving `catalog` under these rules for who may use it, with no
/// budget, no idle window and no wait.
#[must_use]
pub(crate) fn guarded(catalog: &str, root: ModelsRoot, access: Access) -> Serving {
    launched_with(catalog, root, open_limits(None).with_access(access))
}

/// Limits under this budget with no idle window and no wait: what a helper
/// that is about something else starts from, and adjusts.
fn open_limits(limit_mib: Option<u32>) -> Limits {
    Limits::new(
        Budget::new(limit_mib),
        IdleWindow::new(Duration::ZERO),
        Wait::new(Duration::ZERO),
    )
}

/// A router serving `catalog` from `root` within these limits.
fn launched_with(catalog: &str, root: ModelsRoot, limits: Limits) -> Serving {
    let parsed = Catalog::parse(catalog).expect("a usable catalog");
    let server =
        Server::located(Some(&stub_binary())).expect("the stub binary is built by cargo test");
    // Where a reload would read from. The helpers that parse a catalog out of
    // a string still name a path, because `bind` takes one: it is the file
    // `reloadable` wrote, or a name nothing put anything at -- in which case
    // a reload reports that it could not read it, which is the truth.
    let source = root.path().join("catalog.toml");
    let router = Router::bind(
        &["127.0.0.1:0".parse().expect("a loopback address")],
        Source {
            catalog: parsed,
            path: source,
        },
        root.path().to_path_buf(),
        server,
        limits,
    )
    .expect("an ephemeral loopback port");

    let router = Arc::new(router);
    let address = router.address();
    // Detached on purpose: `serve` never returns, so there is nothing to join.
    // The accept loop outlives the test; what must not outlive it is the
    // children, which `Serving`'s Drop ends.
    let serving = Arc::clone(&router);
    thread::spawn(move || serving.serve());
    Serving {
        address,
        router,
        _root: root,
        source: None,
    }
}
