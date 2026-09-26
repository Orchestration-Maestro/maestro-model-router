//! The table itself: which entries have a slot, and how one is reached.
//!
//! Split from `slots` when that module grew past the size gate, along the
//! seam reload put there. Before it, the table was a fact rather than a
//! subject -- built once from the catalog and never touched again -- and
//! everything in `slots` was about what happens *in* a slot. A catalog that
//! can be re-read makes the set of keys a thing with its own rules, and
//! those rules are here: who may add a key, who may drop one, and the lock
//! order every reader keeps. The type holding the table is defined here too,
//! beside those rules, now that the door above it only declares.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use crate::admission::Budget;
use crate::catalog::Catalog;
use crate::queue::Wait;
use crate::voice::Voice;

use super::super::loaded::Slot;
use super::lease::Freed;
use super::queue::Queue;

/// Every entry's slot, and the budget they compete for.
pub(in super::super) struct Slots {
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
    pub(super) by_id: RwLock<HashMap<String, Arc<Slot>>>,
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
    pub(super) admission: Mutex<Queue>,
    /// Rung whenever a model may have stopped being busy.
    pub(super) freed: Freed,
    /// How many requests are in line for room, readable without waiting
    /// behind a load that holds the admission lock.
    pub(super) in_line: AtomicUsize,
    pub(super) budget: Budget,
    pub(super) wait: Wait,
    /// Where a load, and an unload to make room, is said.
    pub(super) voice: Voice,
}

impl Slots {
    /// One slot per entry the catalog carries, all of them empty.
    pub(in super::super) fn new(
        catalog: &Catalog,
        budget: Budget,
        wait: Wait,
        voice: Voice,
    ) -> Self {
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
            in_line: AtomicUsize::new(0),
            budget,
            wait,
            voice,
        }
    }

    /// Ends every child, and forgets them.
    ///
    /// Every slot is emptied first and the children dropped afterwards, so
    /// no slot's guard is held while a process is being killed and waited
    /// for -- the same rule [`take_if_idle`](super::super::loaded::take_if_idle)
    /// keeps, for the same reason.
    pub(in super::super) fn clear(&self) {
        let taken: Vec<_> = self
            .by_id
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter_map(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).take())
            .collect();
        drop(taken);
    }

    /// Holds the admission lock, so a caller can serialise with loading.
    ///
    /// Reload takes this: swapping the catalog while a load is deciding
    /// against the old one would admit an entry under one set of numbers and
    /// insert it under another.
    pub(in super::super) fn admitting(&self) -> MutexGuard<'_, Queue> {
        self.admission
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Gives the new catalog's entries slots, and forgets what it dropped.
    ///
    /// A dropped entry that still holds a child keeps its slot. The child is
    /// a process this router is responsible for ending, and a slot removed
    /// from the map is a handle nothing will ever take again -- the process
    /// would answer until the router itself ended. It goes when the sweep or
    /// an eviction empties it, and until then the entry is gone from the
    /// catalog while its process is not, which `reload` reports.
    ///
    /// The caller holds the admission lock, and hands it in: a request that
    /// waited for room through this is told so when it next looks, and is
    /// rung for so that it looks at once.
    pub(in super::super) fn resync(
        &self,
        admitting: &mut MutexGuard<'_, Queue>,
        catalog: &Catalog,
    ) {
        admitting.reloads += 1;
        self.freed.ring();
        let mut by_id = self.by_id.write().unwrap_or_else(PoisonError::into_inner);
        for entry in &catalog.entries {
            by_id
                .entry(entry.id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(None)));
        }
        by_id.retain(|id, slot| {
            catalog.entry(id).is_some()
                || slot
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some()
        });
    }

    /// The slot for an entry, if the table still has one.
    ///
    /// Cloned out of the map so the map's lock is released before the slot's
    /// is taken -- the lock order the whole module depends on, and the reason
    /// a load of one entry does not block a request for another.
    ///
    /// `None` when a reload dropped the entry after the caller looked it up.
    /// Every caller did so in a catalog it holds, and most hold no admission
    /// lock, so a `resync` can land between the look and this. It only ever
    /// drops a slot that was empty, so a caller that reads finds nothing
    /// loaded, which is the truth; only `admit` has to refuse.
    pub(in super::super) fn slot(&self, id: &str) -> Option<Arc<Slot>> {
        self.by_id
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .map(Arc::clone)
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fmt::Write as _;
    use std::path::Path;
    use std::time::Duration;

    use super::super::super::head::AllowedRoom;
    use super::super::admit::Asked;
    use super::*;
    use crate::launch::{Failure, Server};

    /// A catalog of on-demand entries, one per id, none of them ever started.
    fn catalog(ids: &[&str]) -> Catalog {
        let mut text = "version = 1\n\n[defaults]\ncontext_size = 4096\n\
                        residency = \"on-demand\"\nmemory_estimate_mib = 512\n\
                        startup_timeout_seconds = 30\n"
            .to_owned();
        for id in ids {
            write!(text, "\n[models.{id}]\npath = \"unused\"\n").expect("a String takes it");
        }
        Catalog::parse(&text).expect("a usable catalog")
    }

    /// Slots for `gemma3` alone, as a reload that dropped `qwen3` leaves them
    /// for a request still holding the catalog from before it.
    fn slots() -> Slots {
        Slots::new(
            &catalog(&["gemma3"]),
            Budget::new(None),
            Wait::new(Duration::ZERO),
            Voice::default(),
        )
    }

    #[test]
    fn a_reader_holding_an_older_catalog_finds_a_dropped_slot_empty() {
        let slots = slots();
        let older = catalog(&["gemma3", "qwen3"]);

        assert!(slots.loaded(&older).is_empty(), "nothing is loaded");
        assert!(slots.let_go("qwen3"), "and nothing is left running");
    }

    #[test]
    fn an_admission_for_a_dropped_slot_is_refused_as_contended() {
        let slots = slots();
        let older = catalog(&["gemma3", "qwen3"]);
        let entry = older.entry("qwen3").expect("the entry the reload dropped");
        let server = Server::located(Some(
            &env::current_exe().expect("this test binary's own path"),
        ))
        .expect("this test binary's own path is a file");

        let asked = Asked {
            entry,
            room: AllowedRoom::Any,
        };
        let refused = slots
            .child(&older, asked, &server, Path::new("/somewhere"))
            .err();

        assert!(
            matches!(&refused, Some(Failure::Contended(message)) if message.contains("reloaded")),
            "a retry is decided under the catalog now serving: {refused:?}"
        );
    }
}
