//! A child handed to a request, and the bell it rings when it is let go.
//!
//! A request waiting for room is waiting for a model to stop being busy, and
//! "busy" is a count of handles: nothing happened at the moment the last
//! reader let go, so the wait polled. A handle that rings as it goes turns the
//! poll into a wake.

use std::ops::Deref;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;

use crate::launch::Child;

/// Rung each time a model may have stopped being busy, so a request waiting
/// for its room looks again at once rather than on a timer.
///
/// A count under a lock of its own rather than the admission lock's condition
/// variable: a relay lets its child go at the end of every request, and one
/// that had to take the admission lock to say so would wait out whichever
/// load held it -- with its caller's connection still open behind it.
#[derive(Debug)]
pub(super) struct Freed {
    rung: Mutex<u64>,
    bell: Condvar,
}

impl Freed {
    pub(super) fn new() -> Self {
        Self {
            rung: Mutex::new(0),
            bell: Condvar::new(),
        }
    }

    /// How often it has rung, read before looking at what is busy: a ring
    /// between the look and the wait then ends the wait at once instead of
    /// being slept through.
    pub(super) fn heard(&self) -> u64 {
        *self.rung.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Rings, waking every request waiting for room.
    pub(super) fn ring(&self) {
        *self.rung.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.bell.notify_all();
    }

    /// Waits until it has rung since `heard`, or until `deadline`.
    pub(super) fn wait(&self, heard: u64, deadline: Instant) {
        let rung = self.rung.lock().unwrap_or_else(PoisonError::into_inner);
        let left = deadline.saturating_duration_since(Instant::now());
        drop(
            self.bell
                .wait_timeout_while(rung, left, |rung| *rung == heard)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

/// A child handed to one request, rung for when that request lets it go.
///
/// Holds the handle the slot invariant in `loaded` is about -- cloned under
/// the slot's lock and nowhere else -- so `Arc::strong_count` still means
/// "somebody is reading from this child". What it adds is the moment that
/// stops being true: the handle goes first and the bell rings after, so a
/// request woken by the ring finds the child idle.
pub(in crate::proxy) struct Lease<'a> {
    child: Option<Arc<Child>>,
    freed: &'a Freed,
}

impl<'a> Lease<'a> {
    pub(super) fn new(child: Arc<Child>, freed: &'a Freed) -> Self {
        Self {
            child: Some(child),
            freed,
        }
    }
}

impl Deref for Lease<'_> {
    type Target = Child;

    fn deref(&self) -> &Child {
        // Taken only by `drop`, after which nothing can deref it.
        self.child
            .as_deref()
            .expect("a lease holds its child until it is dropped")
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        drop(self.child.take());
        self.freed.ring();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_wait_nothing_rings_for_lasts_until_its_deadline() {
        let freed = Freed::new();
        let started = Instant::now();
        freed.wait(freed.heard(), started + Duration::from_millis(200));
        assert!(
            started.elapsed() >= Duration::from_millis(200),
            "waited {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_ring_ends_a_wait_at_once() {
        let freed = Arc::new(Freed::new());
        let heard = freed.heard();
        let ringer = Arc::clone(&freed);
        let ringing = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            ringer.ring();
        });

        let started = Instant::now();
        freed.wait(heard, started + Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "woken by the ring rather than the deadline, after {:?}",
            started.elapsed()
        );
        ringing.join().expect("the ring was made");
    }

    #[test]
    fn a_ring_already_heard_does_not_end_the_next_wait() {
        let freed = Freed::new();
        freed.ring();
        let started = Instant::now();
        freed.wait(freed.heard(), started + Duration::from_millis(200));
        assert!(
            started.elapsed() >= Duration::from_millis(200),
            "a waiter that heard the last ring sleeps until the next, rather \
             than spinning on the old one; waited {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_ring_made_before_the_wait_is_not_slept_through() {
        let freed = Freed::new();
        let heard = freed.heard();
        freed.ring();

        let started = Instant::now();
        freed.wait(heard, started + Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the ring between the look and the wait ended it, after {:?}",
            started.elapsed()
        );
    }
}
