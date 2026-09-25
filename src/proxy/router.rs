//! The type a caller holds: the public listeners, and what serving them shares.
//!
//! What a caller must know to use it is in `proxy`, which re-exports it.

use std::fmt;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread;

use super::permits::Permits;
use super::reaper::{self, Stop};
use super::shared::{Shared, Source};
use super::slots::Slots;
use super::{listen, residents};
use crate::idle::Limits;
use crate::launch::{Failure, Server};

/// The public listeners, and everything a request needs to be answered.
pub struct Router {
    listeners: Vec<TcpListener>,
    /// The first of `addresses`, kept apart so asking for it cannot fail.
    address: SocketAddr,
    /// Where each listener ended up, read once when it was bound.
    addresses: Vec<SocketAddr>,
    shared: Arc<Shared>,
}

/// The addresses a router answers on; what it shares is a lock and a table
/// of children, which say nothing a caller could act on.
impl fmt::Debug for Router {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Router")
            .field("addresses", &self.addresses)
            .finish_non_exhaustive()
    }
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
        let (address, addresses) = listen::assigned(&listeners)?;

        let Source { catalog, path } = catalog;
        let slots = Slots::new(&catalog, limits.budget, limits.wait, limits.voice.clone());

        Ok(Self {
            listeners,
            address,
            addresses,
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
                permits: Permits::new(limits.connections),
                stop: Stop::new(),
                voice: limits.voice,
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
    /// that asked for one; [`Router::addresses`] is the rest. There is always
    /// one: [`Router::bind`] refuses an empty list rather than binding nothing.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Every address the router is listening on, in the order asked for.
    #[must_use]
    pub fn addresses(&self) -> Vec<SocketAddr> {
        self.addresses.clone()
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
