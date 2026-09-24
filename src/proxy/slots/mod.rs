//! Which child is loaded for which entry, and what has to go to make room.
//!
//! One type, holding the part of serving that is about memory rather than
//! about HTTP. A caller asks for the child that serves an entry and gets one,
//! or gets told why not; whether that meant finding a running process,
//! starting one, or ending somebody else's first is this module's business
//! and nothing else's.
//!
//! The policy itself is not here. `admission` decides what may be loaded from
//! four values and no machine at all; this acts on that decision, which is the
//! half that kills processes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::admission::Budget;
use crate::catalog::{Catalog, Entry};
use crate::launch::{Failure, Server};
use crate::queue::Wait;

use super::loaded::{Slot, Take, live_child, take_if_idle};

mod lease;
mod room;
mod start;
mod sweep;
mod table;
mod view;

pub(in crate::proxy) use lease::Lease;
pub(in crate::proxy) use room::Queue;
pub(in crate::proxy) use start::say;

use lease::Freed;

/// Every entry's slot, and the budget they compete for.
pub(super) struct Slots {
    /// One slot per catalog entry, which [`Slots::resync`] adds to.
    ///
    /// The map is behind a lock and its values behind `Arc`s, which is a
    /// direct consequence of reload: the set of keys was fixed for the life
    /// of the router, and a catalog that can be re-read makes it not. The
    /// `Arc` is what keeps the old property that mattered -- a caller clones
    /// the handle under a read lock and releases the map before touching the
    /// slot, so a request for one entry still proceeds while another entry is
    /// loading. Only `resync` takes the write lock, and only to add and drop
    /// keys.
    by_id: RwLock<HashMap<String, Arc<Slot>>>,
    /// Serialises starting children, and keeps the line of requests waiting
    /// for room.
    ///
    /// Two loads at once compete for the same memory, so admitting one at a
    /// time is the correct behaviour rather than a limitation: a decision made
    /// while another load is in flight is a decision about a machine state
    /// that no longer holds. Waiting for room is not loading, and is done with
    /// this let go -- see `room`.
    ///
    /// Taken before any slot lock and never held across a relay, which is the
    /// whole deadlock argument: one lock order, so no cycle.
    admission: Mutex<Queue>,
    /// Rung whenever a model may have stopped being busy.
    freed: Freed,
    budget: Budget,
    wait: Wait,
}

impl Slots {
    /// One slot per entry the catalog carries, all of them empty.
    pub(super) fn new(catalog: &Catalog, budget: Budget, wait: Wait) -> Self {
        Self {
            by_id: RwLock::new(
                catalog
                    .entries
                    .iter()
                    .map(|entry| (entry.id.clone(), Arc::new(Mutex::new(None))))
                    .collect(),
            ),
            admission: Mutex::new(Queue::default()),
            freed: Freed::new(),
            budget,
            wait,
        }
    }

    /// The child serving this entry, started if there is room for it.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when a child cannot be started, does not become
    /// ready, or is refused for want of room.
    pub(super) fn child(
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

    /// Ends every child, and forgets them.
    ///
    /// Every slot is emptied first and the children dropped afterwards, so
    /// no slot's guard is held while a process is being killed and waited
    /// for -- the same rule [`take_if_idle`] keeps, for the same reason.
    /// Which entries are loaded right now, by id.
    ///
    /// Ids rather than handles, deliberately: the slot invariant in
    /// [`super::loaded`] is a rule about where an `Arc` may be cloned, and
    /// handing out references to list what is running is exactly what it
    /// warns against. A caller asking this wants to report, not to serve.
    pub(super) fn loaded(&self, catalog: &Catalog) -> Vec<String> {
        self.snapshot(catalog, |entry, _| entry.id.clone())
    }

    /// The child already running for this entry, if there is a live one.
    ///
    /// The fast path, and the common one. No admission lock is taken, so a
    /// request for a loaded model waits on nothing but its own slot.
    fn running(&self, entry: &Entry) -> Option<Lease<'_>> {
        live_child(&self.slot(&entry.id)).map(|child| Lease::new(child, &self.freed))
    }

    /// Starts a child for this entry, unloading what has to go first.
    ///
    /// The slow path. The admission lock is held for each decision and for
    /// the load, so two requests cannot each read a machine state the other
    /// is about to change, and let go while this waits for room. Whether the
    /// entry was started meanwhile is asked again under it, by `room_for`,
    /// because another request may have started this very entry.
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
        let (_admitting, found) = self.room_for(admitting, catalog, entry, root);
        if let Some(child) = found? {
            return Ok(child);
        }

        let loaded = self.start(entry, server, root)?;
        // The handed-out handle and the slot's own come into existence
        // together under the lock, which is the slot invariant in `loaded`.
        // The handle is bound before it is locked: the map's lock has to be
        // released before the slot's is taken, which a temporary would keep
        // alive for the whole statement.
        let handle = self.slot(&entry.id);
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
    /// Each is taken by [`take_if_idle`](super::loaded::take_if_idle), which
    /// re-reads the busy signal for the reason [`Slots::held`] records.
    ///
    /// # Errors
    ///
    /// Returns the entry that had gained a reader, having unloaded whatever
    /// it reached before that one. Those were idle when they were taken, so
    /// ending them was allowed; what is lost is the work of starting them
    /// again, which is the price of not silently overcommitting. Naming the
    /// blocker is what lets the refusal say which model is holding the room,
    /// rather than only that something is.
    fn unload<'a>(&self, ids: &'a [String]) -> Result<(), &'a str> {
        for id in ids {
            match take_if_idle(&self.slot(id), |_| true) {
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
