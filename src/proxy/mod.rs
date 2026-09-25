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
//! [`Router::bind`] takes [`Limits`]: a memory budget and
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

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread;

use crate::idle::Limits;
use crate::launch::{Failure, Server};

mod access;
mod answer;
mod body;
mod endpoint;
mod head;
mod listen;
mod loaded;
mod metrics;
mod reaper;
mod refusal;
mod relay;
mod reply;
mod residents;
mod shared;
mod slots;

pub use access::Access;
pub use listen::{ASSIGNED_WITHIN, await_assigned};
use reaper::Stop;
use shared::Shared;
pub use shared::Source;
use slots::Slots;

/// The public listeners, and everything a request needs to be answered.
#[derive(Debug)]
pub struct Router {
    listeners: Vec<TcpListener>,
    shared: Arc<Shared>,
}

impl Router {
    /// Reserves every public port, in the order given.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when no address was given, when one is a
    /// wildcard, or when one cannot be bound. `listen` holds the reasoning.
    pub fn bind(
        addresses: &[SocketAddr],
        catalog: Source,
        root: PathBuf,
        server: Server,
        limits: Limits,
    ) -> Result<Self, Failure> {
        let listeners = listen::reserve(addresses)?;

        let Source { catalog, path } = catalog;
        let slots = Slots::new(&catalog, limits.budget, limits.wait);

        Ok(Self {
            listeners,
            shared: Arc::new(Shared {
                catalog: RwLock::new(Arc::new(catalog)),
                source: path,
                root,
                server,
                slots,
                resident_failures: Mutex::new(Vec::new()),
                idle_window: limits.idle_window,
                access: limits.access,
                stall: limits.stall,
                permits: listen::Permits::new(limits.connections),
                stop: Stop::new(),
            }),
        })
    }

    /// Which entries hold a child, by identifier, in catalog order.
    ///
    /// Names rather than handles, deliberately. Without this, "the resident
    /// was loaded at startup" cannot be observed at all: any request that
    /// would reveal the child is also a request that would have started it.
    /// Why it returns names is in `Slots::loaded_ids`, and it is the slot
    /// invariant rather than a preference.
    #[must_use]
    pub fn loaded(&self) -> Vec<String> {
        self.shared.slots.loaded_ids(&self.shared.catalog())
    }

    /// How many requests are waiting in line for room right now.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.shared.slots.waiting()
    }

    /// Residents the startup loader could not load, as `id: reason`.
    ///
    /// Empty when every resident loaded, and empty before [`Router::serve`]
    /// has run: nothing is attempted until there is something to serve.
    #[must_use]
    pub fn resident_failures(&self) -> Vec<String> {
        self.shared
            .resident_failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Where the router is listening, as the operating system assigned it.
    ///
    /// The first address asked for, which is the whole answer for a caller
    /// that asked for one; [`Router::addresses`] is the rest.
    ///
    /// # Panics
    ///
    /// If the router has no listener, which cannot happen: [`Router::bind`]
    /// refuses an empty list rather than binding nothing.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        *self
            .addresses()
            .first()
            .expect("a bound router has an address")
    }

    /// Every address the router is listening on, in the order asked for.
    ///
    /// # Panics
    ///
    /// If a listener has no address, which `listen` explains cannot happen.
    #[must_use]
    pub fn addresses(&self) -> Vec<SocketAddr> {
        listen::addresses(&self.listeners)
    }

    /// Stops every child this router started, and forgets them.
    ///
    /// A child still relaying a response is held by that relay and stops when
    /// it finishes, which is the behaviour a caller wants: this ends the
    /// router's claim on its children, not the answer somebody is reading.
    ///
    /// Also ends idle unloading for the rest of this router's life: the
    /// reaper thread, if one was ever spawned, exits the moment this signals
    /// it rather than sleeping out its interval, and nothing here spawns a
    /// replacement. A model that goes idle after this point is held until
    /// something else unloads it.
    ///
    /// The next request for an entry starts a fresh child, so this is safe to
    /// call on a router that goes on serving.
    pub fn stop(&self) {
        self.shared.slots.clear();
        self.shared.stop.signal();
    }

    /// Accepts connections until the process ends.
    ///
    /// One thread per connection, because a streamed response occupies its
    /// thread for as long as the answer takes and this router serves one
    /// machine.
    ///
    /// Residents load on a thread of their own and the accept loops start
    /// immediately, so the router answers while they load. Loading first
    /// would be smaller by a thread and is refused: [`Router::bind`] already
    /// reserved the ports, so a caller connects successfully into the kernel's
    /// backlog and then waits with nothing to tell it why. A resident
    /// carrying the default startup budget would make that a five-minute
    /// silence from something that looks like a live router.
    ///
    /// What the thread buys is every answer that needs no child: the model
    /// list, a refusal, a route that does not exist. It does not buy the
    /// first request to another entry, which finds nothing loaded, enters
    /// admission, and waits there for the resident's start to return. The
    /// wait is bounded by that entry's startup budget rather than removed,
    /// so the silence moved from the listener to the first load.
    pub fn serve(&self) {
        let loading = Arc::clone(&self.shared);
        thread::spawn(move || residents::load(&loading));

        // No window, no thread -- a machine that has not asked for this pays
        // nothing for it, not even a sleeping thread.
        if self.shared.idle_window.duration().is_some() {
            let reaping = Arc::downgrade(&self.shared);
            thread::spawn(move || reaper::run(&reaping));
        }

        listen::accept(&self.listeners, &self.shared);
    }
}
