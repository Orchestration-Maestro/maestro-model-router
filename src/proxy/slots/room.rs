//! Making room for a model, and waiting in turn when the room is held.
//!
//! Moved out of `mod.rs` when the waiting arrived and the module-size gate
//! said so. What lives here is one question -- can this entry be started now,
//! and if not, is that permanent? -- and the wait that asks it again.
//!
//! Two refusals that look alike and are not. A model larger than the whole
//! budget will never fit, however long anybody waits, and is refused at once.
//! A model whose room is held by something still answering will fit as soon as
//! that answer ends, and waiting for it is the difference between a router
//! that queues and one that tells every caller to write a retry loop.
//!
//! The wait does not hold the admission lock. It used to, which kept waiting
//! requests in order but queued every other admission behind them -- a model
//! with room to spare waited for as long as someone else waited for theirs.
//! Now a request that needs room takes a place in line and sleeps until a
//! model is let go; one that fits outright goes ahead, and one that would
//! take room goes in the order it asked.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::admission::{Decision, Wanted};
use crate::catalog::{Catalog, Entry};
use crate::launch::{Failure, Server};

use super::lease::Lease;
use super::{Slots, say};

/// The requests waiting for room, oldest first, under the admission lock.
#[derive(Debug, Default)]
pub(in crate::proxy) struct Queue {
    waiting: VecDeque<u64>,
    issued: u64,
    /// How often the catalog has been swapped, so a request that waited
    /// through a reload can tell.
    pub(super) reloads: u64,
}

impl Queue {
    /// Whether a request may take room now: nobody who asked before it is
    /// still waiting. One not yet in line may only when nobody is.
    fn may_take(&self, ticket: Option<u64>) -> bool {
        self.waiting
            .front()
            .is_none_or(|first| Some(*first) == ticket)
    }

    /// Puts a request in line, once.
    fn join(&mut self, ticket: &mut Option<u64>) {
        if ticket.is_none() {
            self.issued += 1;
            self.waiting.push_back(self.issued);
            *ticket = Some(self.issued);
        }
    }

    /// Takes a request out of line, saying whether it was in it.
    fn leave(&mut self, ticket: Option<u64>) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|waiting| Some(*waiting) != ticket);
        self.waiting.len() != before
    }
}

/// What one look at the room came to.
enum Room {
    /// There is room now, having unloaded whatever had to go.
    Made,
    /// The room is held, by the reader or the earlier request this says.
    Held(String),
}

impl Slots {
    /// Makes room for this entry, waiting for it in turn if the wait allows.
    ///
    /// Takes the admission lock and hands it back, held again, with either
    /// the child another request started meanwhile or nothing -- in which
    /// case the room is made and the caller starts the child under the lock.
    /// Between looks the lock is let go, so an admission that fits outright
    /// is not held behind this one.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure::Refused`] when the entry cannot fit at all, and a
    /// [`Failure::Contended`] when the wait expired with its room still held,
    /// or the catalog was reloaded under it. The difference is what a caller
    /// should do next, and the proxy carries it as a `Retry-After` on the ones
    /// a retry can fix.
    pub(super) fn room_for<'a>(
        &'a self,
        mut queue: MutexGuard<'a, Queue>,
        catalog: &Catalog,
        entry: &Entry,
        root: &Path,
    ) -> (MutexGuard<'a, Queue>, Result<Option<Lease<'a>>, Failure>) {
        let deadline = Instant::now() + self.wait.duration();
        let arrived_under = queue.reloads;
        let mut ticket = None;
        let outcome = loop {
            // Heard before looking, so a model let go between the look and
            // the wait ends the wait instead of being slept through.
            let heard = self.freed.heard();
            if queue.reloads != arrived_under {
                break Err(Failure::Contended(format!(
                    "the catalog was reloaded while '{}' waited for room; ask \
                     again, and it is decided under the catalog now serving",
                    entry.id
                )));
            }
            if let Some(child) = self.running(entry) {
                break Ok(Some(child));
            }
            // Before the decision, because the decision ends processes and
            // this does not. A stale path would otherwise unload the
            // operator's warm model and then answer 502, leaving neither.
            if let Err(failure) = Server::model_file(entry, root) {
                break Err(failure);
            }
            let held = match self.look(catalog, entry, queue.may_take(ticket)) {
                Ok(Room::Made) => break Ok(None),
                Ok(Room::Held(held)) => held,
                Err(failure) => break Err(failure),
            };
            if Instant::now() >= deadline {
                break Err(refused(entry, &held, self.wait.duration()));
            }
            queue.join(&mut ticket);
            drop(queue);
            self.freed.wait(heard, deadline);
            queue = self
                .admission
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
        };
        // Leaving the line lets the next in it look again, so it is rung for.
        if queue.leave(ticket) {
            self.freed.ring();
        }
        (queue, outcome)
    }

    /// One look at the room, unloading what has to go when this request may.
    fn look(&self, catalog: &Catalog, entry: &Entry, may_take: bool) -> Result<Room, Failure> {
        // Asked on every look, not once. What a wait waits for is memory
        // being released, and the device is the only thing that knows when
        // that has actually happened: a child that has exited frees its pages
        // whether or not the ledger has caught up.
        let device_free_mib = self.budget.probe().device().map(|device| device.free_mib());
        match self
            .budget
            .admit(&self.held(catalog), &Wanted::of(entry), device_free_mib)
        {
            Decision::Fits => Ok(Room::Made),
            // Room that is there for the taking, but not this request's to
            // take while an earlier one waits for room of its own.
            Decision::Unload(_) if !may_take => Ok(Room::Held(format!(
                "'{}' is in line behind a request that asked for room first",
                entry.id
            ))),
            Decision::Unload(ids) => {
                say(&format!(
                    "{}: unloading {} to make room",
                    entry.id,
                    ids.join(", ")
                ));
                match self.unload(&ids) {
                    Ok(()) => Ok(Room::Made),
                    // Something started reading the model whose room this
                    // wanted, between the decision and the taking.
                    // `tests/eviction.rs` tells this apart from a snapshot
                    // that already saw it busy by "reached first", because
                    // what the two leave behind differs.
                    Err(blocker) => Ok(Room::Held(format!(
                        "'{}' needs room held by '{blocker}', which a request \
                         reached first",
                        entry.id
                    ))),
                }
            }
            // Something holding the room is on-demand and busy, so it becomes
            // a candidate the moment its reader is done.
            Decision::Blocked(message) => Ok(Room::Held(message)),
            // Larger than the whole budget, or held by residents that never
            // become candidates. Waiting changes nothing.
            Decision::Refuse(message) => Err(Failure::Refused(message)),
        }
    }
}

/// The refusal a caller sees when the wait ran out.
///
/// Carries what was holding the room, because "try again" without a subject
/// tells an operator nothing about whether trying again is worth it. Names the
/// wait too: a caller held for a minute should not be left wondering whether
/// it waited at all.
///
/// Contended rather than refused, and the distinction is the whole of what a
/// caller does next. Every path that reaches here was held by something that
/// will finish -- a reader that got there first, an on-demand entry still
/// busy, an earlier request in line -- so the answer changes on its own and a
/// retry is the right response. The proxy sends `Retry-After` on this alone.
fn refused(entry: &Entry, held: &str, waited: Duration) -> Failure {
    if waited.is_zero() {
        return Failure::Contended(format!(
            "{held}; this may succeed on a retry, or set \
             MAESTRO_ADMISSION_WAIT_SECONDS to wait for the room"
        ));
    }
    Failure::Contended(format!(
        "{held}; '{}' waited {}s and the room did not free up",
        entry.id,
        waited.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_line_lets_anyone_take_room() {
        assert!(Queue::default().may_take(None));
    }

    #[test]
    fn only_the_oldest_in_line_may_take_room() {
        let mut queue = Queue::default();
        let (mut first, mut second) = (None, None);
        queue.join(&mut first);
        queue.join(&mut second);

        assert!(queue.may_take(first), "the oldest may");
        assert!(!queue.may_take(second), "a later one may not");
        assert!(!queue.may_take(None), "nor one not yet in line");
    }

    #[test]
    fn joining_twice_keeps_one_place_and_leaving_hands_it_on() {
        let mut queue = Queue::default();
        let (mut first, mut second) = (None, None);
        queue.join(&mut first);
        queue.join(&mut first);
        queue.join(&mut second);
        assert_ne!(first, second, "each request its own place");

        assert!(queue.leave(first), "the oldest was in line");
        assert!(queue.may_take(second), "and the next is now first");
        assert!(!queue.leave(first), "and is not in it twice");
        assert!(!queue.leave(None), "a request never in line leaves nothing");
    }
}
