//! The table itself: which entries have a slot, and how one is reached.
//!
//! Split from `slots` when that module grew past the size gate, along the
//! seam reload put there. Before it, the table was a fact rather than a
//! subject -- built once from the catalog and never touched again -- and
//! everything in `slots` was about what happens *in* a slot. A catalog that
//! can be re-read makes the set of keys a thing with its own rules, and
//! those rules are here: who may add a key, who may drop one, and the lock
//! order every reader keeps.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::catalog::Catalog;

use super::super::loaded::Slot;
use super::{Queue, Slots};

impl Slots {
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

    /// The slot for an entry, which exists because the catalog named it.
    ///
    /// Cloned out of the map so the map's lock is released before the slot's
    /// is taken -- the lock order the whole module depends on, and the reason
    /// a load of one entry does not block a request for another.
    ///
    /// # Panics
    ///
    /// If the entry has no slot, which cannot happen: every caller reached
    /// this by looking the entry up in a catalog, and `resync` gives every
    /// entry of every catalog it accepts a slot before that catalog is
    /// served.
    pub(in super::super) fn slot(&self, id: &str) -> Arc<Slot> {
        Arc::clone(
            self.by_id
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(id)
                .expect("one slot per catalog entry"),
        )
    }
}
