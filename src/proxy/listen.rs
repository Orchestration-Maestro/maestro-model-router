//! Which addresses this router answers on, and who accepts on each.
//!
//! Split from the module beside it when `proxy` reached the module-size gate,
//! along the seam the gate exposed: `proxy` carries the type a caller holds,
//! and this carries the ports that exist and the threads that accept on them.
//!
//! More than one address is the point of this module. A router reached over a
//! bridge as well as from the machine it runs on binds both, and every
//! listener hands its connections to the same `Shared` -- so the catalog is
//! read once, a child is started once, and the memory budget is spent once. A
//! second process per address would spend it twice and load the same model
//! into the same card behind its own bookkeeping.
//!
//! A wildcard is refused. `0.0.0.0` and `::` bind every interface the machine
//! has now and every one it gains later, which is a reach no operator stated
//! and the failure the old loopback-only rule existed to prevent. An address
//! that names an interface is bound, and whether anything can reach it is
//! then the operating system's answer -- a route, a firewall -- rather than a
//! rule of this router's. That is the whole of the change: serving is still
//! one machine's business, and which interfaces that machine answers on is
//! stated at the call rather than assumed here.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread;

use super::{Shared, answer};
use crate::launch::Failure;

/// Reserves every address given, in the order given.
///
/// # Errors
///
/// Returns a [`Failure`] when no address was given, when one of them is a
/// wildcard, or when one of them cannot be bound.
pub(super) fn reserve(addresses: &[SocketAddr]) -> Result<Vec<TcpListener>, Failure> {
    if addresses.is_empty() {
        return Err(Failure::Unavailable(
            "refusing to serve: no address to listen on was given".to_owned(),
        ));
    }

    addresses.iter().map(one).collect()
}

/// Reserves one address, refusing a wildcard before the kernel is asked.
fn one(address: &SocketAddr) -> Result<TcpListener, Failure> {
    if address.ip().is_unspecified() {
        return Err(Failure::Unavailable(format!(
            "refusing to bind {address}: a wildcard is every interface this \
             machine has and every one it gains later, which is a reach \
             nothing stated -- name the interface to serve on instead"
        )));
    }

    TcpListener::bind(address)
        .map_err(|error| Failure::Unavailable(format!("cannot bind {address}: {error}")))
}

/// Where every listener ended up, in the order they were reserved.
///
/// What was asked for and what was bound are not the same thing: an address
/// carrying port zero is a request for whichever port is free, and only the
/// listener knows which one that turned out to be.
///
/// # Panics
///
/// If a listener has no address, which cannot happen: every listener here was
/// returned by a bind that succeeded.
pub(super) fn addresses(listeners: &[TcpListener]) -> Vec<SocketAddr> {
    listeners
        .iter()
        .map(|listener| {
            listener
                .local_addr()
                .expect("a bound listener has an address")
        })
        .collect()
}

/// Accepts on every listener until the process ends.
///
/// The caller's thread takes the last listener rather than waiting on threads
/// doing the work it could do: one address, which is the common case, costs
/// no thread at all, exactly as it did before a second one was possible.
///
/// Scoped, because a listener is borrowed from the router that owns it. The
/// scope ends when every accept loop ends, which is never, so this does not
/// return -- the same promise `Router::serve` already made.
pub(super) fn accept(listeners: &[TcpListener], shared: &Arc<Shared>) {
    let Some((last, rest)) = listeners.split_last() else {
        return;
    };

    thread::scope(|accepting| {
        for listener in rest {
            accepting.spawn(|| on(listener, shared));
        }
        on(last, shared);
    });
}

/// Accepts on one listener until the process ends.
fn on(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming().flatten() {
        let shared = Arc::clone(shared);
        thread::spawn(move || {
            // A failed answer is a caller that hung up, which is its own
            // business. The next connection is what matters.
            drop(answer::to(&shared, stream));
        });
    }
}
