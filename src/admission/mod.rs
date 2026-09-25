//! Deciding what may be loaded, and what must be unloaded first.
//!
//! This module touches no process and no socket. It takes a budget, what is
//! loaded now, what is wanted, and what the device has free, and returns a
//! decision; acting on that decision belongs to the caller. That separation
//! is deliberate: the policy is the part of eviction that is hard to get
//! right, and keeping it a pure function means it can be driven exhaustively
//! from a handful of values without a machine, a model, or a clock that has
//! to be waited on. The one place the machine is asked is `Budget`'s own
//! construction, in `budget.rs`, and it is asked before any of this runs.
//!
//! Two rules shape every decision here.
//!
//! A **candidate** is a loaded model the router may unload: on-demand, and
//! with nothing reading from it. A resident model is never a candidate, which
//! is what residency means. A busy model is never a candidate either, because
//! unloading one kills the process answering a request that is still being
//! read -- and the caller sees a stream stop early, which is indistinguishable
//! from a model that finished.
//!
//! **The coldest candidate goes first.** When more than one could be unloaded,
//! the one that answered longest ago is chosen, because it is the one least
//! likely to be asked for again in the next moment.
//!
//! Two questions are asked of every start, and both have to say yes. The
//! **ledger** is the budget: a ceiling on what the loaded models cost, where
//! a model costs its catalog estimate until it has been measured and the
//! larger of the two afterwards. The **device** is what the machine reports
//! free right now, which counts everything else running on it; a model that
//! puts its weights on the device has to fit in that room too, whatever the
//! ledger says. Where the machine cannot be asked, the device question is
//! not asked, and the ledger decides alone as it always did.

mod budget;
mod decision;
mod room;
mod subject;

pub use budget::Budget;
pub use decision::Decision;
pub use subject::{Loaded, Wanted};
