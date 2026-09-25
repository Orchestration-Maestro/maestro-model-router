//! Turning one catalog entry into a running server, and stopping it again.
//!
//! What a caller must know to use this correctly is four things.
//!
//! [`Server::start`] blocks. It returns once the child has finished loading
//! and will answer, or once it has failed, so a caller cannot forget to wait
//! and then wonder why the first request was refused.
//!
//! Every failure names the entry it came from. A message saying only that a
//! health check failed sends the reader back to the catalog to guess which of
//! four models it was about.
//!
//! A child binds loopback only, whatever the router in front of it binds. A
//! child is reached by that router and by nothing else, so an address anyone
//! else could reach would widen what is exposed without widening what is
//! usable. Which interfaces the router itself answers on is its own decision,
//! stated at its call; this one is not re-decided with it.
//!
//! A child is not restarted. Detecting that one exited is [`Child::check`];
//! deciding what to do about it belongs to the slice that has a request in
//! flight to keep waiting, because until then there is nothing to protect.
//!
//! Locating the binary, translating an entry into a command line, choosing a
//! port, polling for readiness, and stopping a process on three operating
//! systems are all implementation and stay inside.

mod binary;
mod child;
mod failure;
mod invocation;
mod output;
mod probe;
mod root;
mod search;
mod server;

// Shared with `memory`, which locates the device tool the same way this
// module locates the server binary: a second copy of the walk would be the
// duplication the gate exists to refuse.
pub use child::{Child, Liveness};
pub use failure::Failure;
pub use output::LineSink;
pub use root::{models_root, models_root_from};
pub(crate) use search::on_search_path;
pub use server::Server;
