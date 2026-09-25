//! Handing a request its child: the one running, or one started once there
//! is room for it.
//!
//! Moved out of `mod.rs` when that file became a door that only declares.
//! What lives here is the path every request takes -- the fast one that finds
//! a running child, the slow one that makes room and starts it -- and the
//! unloading that making room does, beside the two readings a caller makes of
//! it: what is loaded, and how many wait in line.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use crate::catalog::{Catalog, Entry};
use crate::launch::{Failure, Server};

use super::super::loaded::{Take, live_child, take_if_idle};
use super::lease::Lease;
use super::table::Slots;

impl Slots {
    /// The child serving this entry, started if there is room for it.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when a child cannot be started, does not become
    /// ready, or is refused for want of room.
    pub(in super::super) fn child(
        &self,
        catalog: &Catalog,
        entry: &Entry,
        server: &Server,
        root: &Path,
    ) -> Result<Lease<'_>, Failure> {
        if let Some(child) = self.running(entry) {
            return Ok(child);
        }
        self.admit(catalog, entry, server, root)
    }

    /// Which entries are loaded right now, by id.
    ///
    /// Ids rather than handles, deliberately: the slot invariant in
    /// [`super::super::loaded`] is a rule about where an `Arc` may be cloned,
    /// and handing out references to list what is running is exactly what it
    /// warns against. A caller asking this wants to report, not to serve.
    pub(in super::super) fn loaded(&self, catalog: &Catalog) -> Vec<String> {
        self.snapshot(catalog, |entry, _| entry.id.clone())
    }

    /// How many requests are waiting in line for room right now.
    pub(in super::super) fn waiting(&self) -> usize {
        self.in_line.load(Ordering::Relaxed)
    }

    /// The child already running for this entry, if there is a live one.
    ///
    /// The fast path, and the common one. No admission lock is taken, so a
    /// request for a loaded model waits on nothing but its own slot.
    pub(super) fn running(&self, entry: &Entry) -> Option<Lease<'_>> {
        let handle = self.slot(&entry.id)?;
        live_child(&handle).map(|child| Lease::new(child, &self.freed))
    }

    /// Starts a child for this entry, unloading what has to go first.
    ///
    /// The slow path. The admission lock is held for each decision and for
    /// the load, so two requests cannot each read a machine state the other
    /// is about to change, and let go while this waits for room. Whether the
    /// entry was started meanwhile is asked again under it, by `room_for`,
    /// because another request may have started this very entry.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure::Contended`] when a reload dropped the entry after
    /// the caller looked it up, as `room_for` does for one that lands while
    /// the request waits: asked again, it is decided under the catalog now
    /// serving.
    fn admit(
        &self,
        catalog: &Catalog,
        entry: &Entry,
        server: &Server,
        root: &Path,
    ) -> Result<Lease<'_>, Failure> {
        let admitting = self
            .admission
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Looked up under the lock `resync` takes, so the slot a child is put
        // in below is still the table's, and not one a reload has forgotten.
        let Some(handle) = self.slot(&entry.id) else {
            return Err(Failure::Contended(format!(
                "the catalog was reloaded before '{}' was admitted; ask again, \
                 and it is decided under the catalog now serving",
                entry.id
            )));
        };
        let (_admitting, found) = self.room_for(admitting, catalog, entry, root);
        if let Some(child) = found? {
            return Ok(child);
        }

        let loaded = self.start(entry, server, root)?;
        // The handed-out handle and the slot's own come into existence
        // together under the lock, which is the slot invariant in `loaded`.
        // The handle was bound before it is locked: the map's lock has to be
        // released before the slot's is taken, which a temporary would keep
        // alive for the whole statement.
        let mut slot = handle.lock().unwrap_or_else(PoisonError::into_inner);
        let child = Arc::clone(&loaded.child);
        *slot = Some(loaded);
        Ok(Lease::new(child, &self.freed))
    }

    /// Unloads the named entries, or names the one that stopped it.
    ///
    /// Taking the `Loaded` out drops the router's `Arc`, and a child whose
    /// last reference goes is killed by its own `Drop`. Done before the wanted
    /// child is started, which is the point: the room has to be free before
    /// something is put in it. Each drop happens here, once its slot's guard
    /// has been released, so a kill that hangs holds the admission lock this
    /// runs under -- which it has to, since the room is not free until the
    /// process is gone -- and nothing else.
    ///
    /// Each is taken by [`take_if_idle`](super::super::loaded::take_if_idle),
    /// which re-reads the busy signal for the reason [`Slots::held`] records.
    /// An entry with no slot left has nothing in it to unload.
    ///
    /// # Errors
    ///
    /// Returns the entry that had gained a reader, having unloaded whatever
    /// it reached before that one. Those were idle when they were taken, so
    /// ending them was allowed; what is lost is the work of starting them
    /// again, which is the price of not silently overcommitting. Naming the
    /// blocker is what lets the refusal say which model is holding the room,
    /// rather than only that something is.
    pub(super) fn unload<'a>(&self, ids: &'a [String]) -> Result<(), &'a str> {
        for id in ids {
            let Some(handle) = self.slot(id) else {
                continue;
            };
            match take_if_idle(&handle, |_| true) {
                Take::Taken(child) => drop(child),
                Take::Busy => return Err(id),
                // Gone already, by a sweep or another admission: the room
                // this wanted is there, which is all this asked for.
                Take::Empty => {}
            }
        }
        Ok(())
    }
}
