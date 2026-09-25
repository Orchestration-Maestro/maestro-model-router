//! Serving every model in the catalog, and relaying each one to a child.
//!
//! Two shapes reach the same children. A dedicated endpoint names its model in
//! the path; the generic one takes it from the request body, which is what an
//! OpenAI-compatible client already sends. `GET /v1/models` is answered from
//! the catalog without starting anything.
//!
//! What a caller must know to use this correctly is seven things.
//!
//! [`Router::bind`] reserves every public port and returns immediately.
//! Nothing is served and no child is started until [`Router::serve`] runs, so
//! a caller can learn its addresses and arrange what it needs first.
//!
//! [`Router::addresses`] reports what the operating system gave, in the order
//! asked for, and [`Router::address`] is the first of them. A caller that
//! asked for port zero -- a test, usually -- has no other way to find out.
//!
//! [`Router::serve`] runs until the process ends. It does not return.
//!
//! [`Router::stop`] ends the children it started, and also ends idle
//! unloading for the rest of this router's life. This exists because a child
//! is a separate process and nothing in the operating system ties its lifetime
//! to this one: a router that is never dropped -- which is every router whose
//! `serve` is still running -- leaves its children alive after the process
//! that started them is gone. That was measured, not assumed: forty-five stub
//! servers outlived the test binaries that started them before this method
//! existed. A caller that ends without calling it leaves them behind, and a
//! router ended by a signal never gets the chance to call it at all.
//!
//! [`Router::bind`] takes [`Limits`](crate::idle::Limits): a memory budget and
//! an idle window, configured independently, and together these are what can
//! end a process with no caller asking to stop it. Under a budget, a request
//! for a model that does not fit unloads the coldest idle one to make room; a
//! model something is reading from is never chosen, and when the only room is
//! held by one of those the request is refused instead. An idle window ends a
//! process a different way: a background thread unloads an on-demand model
//! nothing has asked for in longer than the window, with no request involved
//! at all. Without a budget nothing is ever unloaded to make room; without a
//! window nothing is ever unloaded for sitting idle.
//!
//! The router binds the addresses it is given and refuses a wildcard: a
//! caller names each interface to answer on -- loopback, a bridge -- and one
//! router serves them all, so a model is loaded once however it is reached.
//! Why `0.0.0.0` and `::` are refused where a named interface is not is in
//! `listen`, which holds that rule.
//!
//! The router reads a request head and copies everything else. That is the
//! decision this module exists to hold: what it does not parse, it cannot
//! buffer, so a streamed response reaches the caller as it arrives rather than
//! when it ends.
//!
//! **This is not a general-purpose HTTP server.** It reads a bounded head,
//! refuses the framing it does not implement, and serves the traffic of the
//! machines its operator named an interface for. Nothing here is hardened
//! against a hostile caller, and putting it where one can reach it is the
//! mistake this paragraph exists to prevent.

mod access;
mod answer;
mod body;
mod endpoint;
mod head;
mod listen;
mod loaded;
mod metrics;
mod permits;
mod reaper;
mod refusal;
mod relay;
mod reply;
mod residents;
mod router;
mod shared;
mod slots;

pub use crate::access::Access;
pub use crate::voice::Voice;
pub use listen::{ASSIGNED_WITHIN, await_assigned};
pub use router::Router;
pub use shared::Source;
