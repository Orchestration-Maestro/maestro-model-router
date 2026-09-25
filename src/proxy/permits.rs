//! How many connections are answered at once.
//!
//! Apart from `listen`, which takes and gives the turns, because `Shared`
//! holds the count and `listen` hands every connection a `Shared`: kept in
//! `listen`, the two would each name the other.

use std::sync::{Condvar, Mutex, PoisonError};

/// How many connections may be answered at once, shared by every listener.
pub(super) struct Permits {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Permits {
    /// This many, and never none: a router that could accept nothing would
    /// be bound and silent, which is worse than either.
    pub(super) fn new(count: usize) -> Self {
        Self {
            free: Mutex::new(count.max(1)),
            freed: Condvar::new(),
        }
    }

    /// Waits until a connection may be answered, and takes that turn.
    pub(super) fn take(&self) {
        let free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        let mut free = self
            .freed
            .wait_while(free, |free| *free == 0)
            .unwrap_or_else(PoisonError::into_inner);
        *free -= 1;
    }

    /// Gives a turn back, waking an accept that is waiting for one.
    pub(super) fn give(&self) {
        *self.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_free_turn_is_taken_at_once() {
        // On a thread with a deadline, because a take that waited for a
        // turn it already had would wait for ever.
        let permits = Arc::new(Permits::new(2));
        let taking = Arc::clone(&permits);
        let (taken, told) = mpsc::channel();
        thread::spawn(move || {
            taking.take();
            taking.take();
            taken.send(()).ok();
        });

        told.recv_timeout(Duration::from_secs(5))
            .expect("both free turns were taken without waiting");
    }
}
