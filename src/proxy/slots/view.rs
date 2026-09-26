//! The read-only projections of what is loaded, moved here when `slots.rs`
//! ran out of room under the module-size gate. No decision lives here: this
//! only looks at what is loaded, through whatever lens a caller needs.

use crate::admission::Loaded as Held;
use crate::catalog::{Catalog, Entry};

use super::super::loaded::{Loaded, busy};
use super::table::Slots;

use std::sync::PoisonError;

impl Slots {
    /// The ceiling models are unloaded to stay under, when there is one.
    pub(in super::super) fn budget_mib(&self) -> Option<u32> {
        self.budget.limit_mib()
    }

    /// Every occupied slot, seen through `project`, in catalog order.
    ///
    /// Each slot is locked in turn, so this is a snapshot of several moments
    /// rather than a reading of one. The admission lock holds it still against
    /// other admissions and nothing else: the fast path takes no admission
    /// lock, so an entry read as idle here can have a reader before the
    /// decision reaches its slot. That is why `Slots::unload` reads the
    /// signal again at the moment it acts instead of trusting this.
    ///
    /// `project` is handed a borrow and never an owned handle, which keeps
    /// every caller clear of the slot invariant in [`super::super::loaded`] --
    /// a rule about where an `Arc` is cloned, which warns by name against
    /// listing what is loaded by handing out references.
    ///
    /// An entry a reload has dropped since `catalog` was read has no slot,
    /// and is not loaded: a reload drops only a slot that was empty.
    pub(super) fn snapshot<T>(
        &self,
        catalog: &Catalog,
        project: impl Fn(&Entry, &Loaded) -> T,
    ) -> Vec<T> {
        catalog
            .entries
            .iter()
            .filter_map(|entry| {
                // Bound before it is locked: the map's lock is released
                // before the slot's is taken, which is the order the module
                // keeps and a temporary would not.
                let handle = self.slot(&entry.id)?;
                let slot = handle.lock().unwrap_or_else(PoisonError::into_inner);
                let held = slot.as_ref()?;
                Some(project(entry, held))
            })
            .collect()
    }

    /// What each loaded entry was measured holding, in catalog order.
    ///
    /// Taken once, as the child becomes ready, and then only read. Put beside
    /// the estimate the catalog declares, it is the drift an operator is
    /// looking for: the two numbers were already both known and the only
    /// place they appeared together was a line on a stdout the service sends
    /// to `/dev/null`.
    ///
    /// The larger of the resident and device sides, which is what the budget's
    /// one number stands for -- and `None` where neither could be read, since
    /// a measurement that did not happen is not a measurement of nothing.
    ///
    /// An entry holding no child is absent rather than zero.
    pub(in super::super) fn memory(&self, catalog: &Catalog) -> Vec<(String, Option<u64>)> {
        self.snapshot(catalog, |entry, held| {
            (entry.id.clone(), held.measured.largest_mib())
        })
    }

    /// What is loaded now, as admission needs to see it.
    ///
    /// Each entry is counted at its estimate or at what it was measured to
    /// hold once loaded, whichever is more, so an under-estimated model is
    /// accounted at its real cost from the moment it is known.
    pub(super) fn held(&self, catalog: &Catalog) -> Vec<Held> {
        self.snapshot(catalog, |entry, held| {
            Held::of(entry, busy(&held.child), held.last_used, &held.measured)
        })
    }

    /// What [`Slots::held`] says, and which of it are guests -- loaded into
    /// free room, the ones admission unloads before any other -- from one
    /// look at each slot.
    ///
    /// One pass rather than two, because the slots do not stand still between
    /// them: an unload on request, the reaper, or a request that finds its
    /// child dead empties a slot without the admission lock. A guest emptied
    /// between a walk for what is held and a walk for the guests would be held
    /// but no guest, ranked as an ordinary model, and a colder chat model could
    /// go first for room the guest had already given back.
    pub(super) fn held_and_guests(&self, catalog: &Catalog) -> (Vec<Held>, Vec<String>) {
        let (held, guests): (Vec<Held>, Vec<Option<String>>) = self
            .snapshot(catalog, |entry, held| {
                (
                    Held::of(entry, busy(&held.child), held.last_used, &held.measured),
                    held.guest.then(|| entry.id.clone()),
                )
            })
            .into_iter()
            .unzip();
        (held, guests.into_iter().flatten().collect())
    }
}
