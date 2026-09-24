//! What `Shared` itself answers, apart from either connection handling in
//! `answer` or the memory bookkeeping in `slots`.
//!
//! Split out when `answer.rs` grew past the module-size gate: these two
//! methods are about `Shared` rather than about answering a connection, and
//! moving them here keeps that gate meaningful rather than merely satisfied.
//!
//! Re-reading the catalog lives here too, for the same reason: it is about
//! what this router is serving rather than about any one request, and the
//! endpoint that triggers it only turns the outcome into a reply.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use super::reaper::Stop;
use super::slots::Slots;
use crate::catalog::{Catalog, Entry};
use crate::idle::IdleWindow;
use crate::launch::{Failure, Server};

/// A catalog and the file it was read from.
///
/// One value rather than two arguments, because neither half is useful
/// without the other once a catalog can be re-read: a parsed catalog with no
/// source cannot be read again, and a path nothing has parsed has not been
/// shown to be a catalog at all.
pub struct Source {
    /// The catalog as it was parsed.
    pub catalog: Catalog,
    /// Where it was parsed from, which `POST /reload` reads again.
    pub path: PathBuf,
}

/// What every connection thread shares.
pub(super) struct Shared {
    /// The catalog being served, which `reload` replaces.
    ///
    /// Behind a lock because it is no longer fixed for the life of the
    /// process, and behind an `Arc` inside it so a reader clones a whole
    /// catalog and lets the lock go rather than holding it across a load. A
    /// request that began under one catalog finishes under it: swapping the
    /// pointer cannot change an entry a caller is already acting on.
    pub(super) catalog: RwLock<Arc<Catalog>>,
    /// The file the catalog was read from, so it can be read again.
    pub(super) source: PathBuf,
    pub(super) root: PathBuf,
    pub(super) server: Server,
    pub(super) slots: Slots,
    /// Residents the startup loader could not load, as `id: reason`.
    ///
    /// Recorded rather than only printed, because the loader runs on a thread
    /// of its own: what it writes to the output is not reachable from the
    /// caller that started serving, and "a resident that cannot load says so"
    /// is a claim that has to be assertable to be a guarantee.
    pub(super) resident_failures: Mutex<Vec<String>>,
    /// How long an on-demand, idle entry may go unused before the reaper
    /// unloads it. `None` means the reaper is never spawned at all.
    pub(super) idle_window: IdleWindow,
    /// Who may use the router, checked before anything else is done.
    pub(super) access: super::Access,
    /// How long a caller may make no progress before it is given up on.
    pub(super) stall: std::time::Duration,
    /// How many connections are answered at once.
    pub(super) permits: super::listen::Permits,
    /// Wakes the reaper the moment [`Router::stop`] is called. See
    /// [`reaper::Stop`] for why a `Weak<Shared>` alone is not enough: the
    /// test harness never drops a `Router`, so nothing would ever end it.
    pub(super) stop: Stop,
}

/// What a reload changed, so the reply can say rather than only succeed.
pub(super) struct Reloaded {
    /// Entries the new catalog has and the old one did not.
    pub(super) added: Vec<String>,
    /// Entries the old catalog had and the new one does not.
    pub(super) removed: Vec<String>,
    /// Entries whose definition changed while a child was already running.
    ///
    /// The running process keeps the arguments it was started with -- there
    /// is no way to change them without ending it, and ending it is what
    /// reload exists to avoid. The new definition applies the next time the
    /// entry is loaded, and until then the catalog and the process disagree.
    /// An operator who is not told that will read the catalog and believe it.
    pub(super) superseded: Vec<String>,
}

impl Shared {
    /// The catalog being served, as it stands now.
    ///
    /// Cloned rather than borrowed, so the lock is released before the caller
    /// does anything with it. A load takes seconds and holds no lock on the
    /// catalog at all; a reload that lands mid-load changes what the *next*
    /// request sees and never what this one is already acting on.
    pub(super) fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// The child serving this entry, started if there is room for it.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when a child cannot be started, does not become
    /// ready, or is refused for want of room.
    pub(super) fn child(&self, entry: &Entry) -> Result<super::slots::Lease<'_>, Failure> {
        self.slots
            .child(&self.catalog(), entry, &self.server, &self.root)
    }

    /// What the catalog carries, for a refusal that can be acted on.
    pub(super) fn known(&self) -> String {
        self.catalog()
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Reads the catalog file again and serves what it now says.
    ///
    /// Nothing that is running is stopped. Slots appear for entries the file
    /// gained and are dropped for entries it lost, unless a lost entry still
    /// holds a child -- that slot stays until the child goes, because
    /// forgetting a running process is how one is orphaned.
    ///
    /// # Errors
    ///
    /// Returns the reason when the file cannot be read or cannot be parsed.
    /// In both cases the catalog that was serving keeps serving: a router
    /// that emptied itself because a file was briefly mid-write would take
    /// every model down over a text editor.
    pub(super) fn reload(&self) -> Result<Reloaded, String> {
        let text = fs::read_to_string(&self.source)
            .map_err(|error| format!("cannot read {}: {error}", self.source.display()))?;
        let parsed = Catalog::parse(&text).map_err(|report| report.to_string())?;

        // Taken before the swap and held across it, so a load cannot be
        // admitted against one catalog and inserted into the slots of
        // another. This is the same lock loads take, in the same order, and
        // it is the whole of the argument that a reload is not a race.
        let mut admission = self.slots.admitting();

        let previous = self.catalog();
        let names = |catalog: &Catalog| -> Vec<String> {
            catalog.entries.iter().map(|e| e.id.clone()).collect()
        };
        let before = names(&previous);
        let after = names(&parsed);

        let added: Vec<String> = after
            .iter()
            .filter(|id| !before.contains(id))
            .cloned()
            .collect();
        let removed: Vec<String> = before
            .iter()
            .filter(|id| !after.contains(id))
            .cloned()
            .collect();

        // An entry whose definition changed under a running child. Compared
        // whole: any field the launcher reads is one the running process was
        // started without, and naming which field changed would be a promise
        // to keep that list in step with the launcher for ever.
        let loaded = self.slots.loaded(&previous);
        let superseded: Vec<String> = loaded
            .into_iter()
            .filter(|id| match (previous.entry(id), parsed.entry(id)) {
                (Some(was), Some(now)) => was != now,
                // A loaded entry the new catalog dropped is superseded too:
                // the process is still answering for a model the operator has
                // removed, which is exactly the disagreement this reports.
                (Some(_), None) => true,
                _ => false,
            })
            .collect();

        self.slots.resync(&mut admission, &parsed);
        *self.catalog.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(parsed);
        drop(admission);

        Ok(Reloaded {
            added,
            removed,
            superseded,
        })
    }
}
