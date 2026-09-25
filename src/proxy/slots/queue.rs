//! The line of requests waiting for room, kept under the admission lock.
//!
//! Moved out of `room` so the table that holds the admission lock and the
//! wait that joins the line can both name it without naming each other.

use std::collections::VecDeque;

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
    pub(super) fn may_take(&self, ticket: Option<u64>) -> bool {
        self.waiting
            .front()
            .is_none_or(|first| Some(*first) == ticket)
    }

    /// Puts a request in line, once.
    pub(super) fn join(&mut self, ticket: &mut Option<u64>) {
        if ticket.is_none() {
            self.issued += 1;
            self.waiting.push_back(self.issued);
            *ticket = Some(self.issued);
        }
    }

    /// Takes a request out of line, saying whether it was in it.
    pub(super) fn leave(&mut self, ticket: Option<u64>) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|waiting| Some(*waiting) != ticket);
        self.waiting.len() != before
    }

    /// How many requests are in line.
    pub(super) fn len(&self) -> usize {
        self.waiting.len()
    }
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
