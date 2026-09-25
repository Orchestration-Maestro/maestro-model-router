//! Where the router listens, and what it refuses to listen on.
//!
//! One process, more than one address. A machine that reaches the router over
//! a bridge and the machine the router runs on are two callers at two
//! addresses, and what they must find is one router: the same catalog, the
//! same children, the same memory budget spent once. Two processes would
//! serve the same catalog and load the model twice, which is the failure
//! these tests exist to rule out.
//!
//! The addresses here are two ephemeral ports on loopback rather than two
//! interfaces, because a test that named an interface would assert about
//! whichever machine it ran on. Two ports is the shape the field uses -- the
//! bridge is given its own -- and it is the shape every platform can bind.

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::support::{MODEL, ModelsRoot, catalog_text, get, request, status, stub_binary};

use maestro_model_router::admission::Budget;
use maestro_model_router::catalog::Catalog;
use maestro_model_router::idle::{IdleWindow, Limits};
use maestro_model_router::launch::{Failure, Server};
use maestro_model_router::proxy::{Router, Source, await_assigned};
use maestro_model_router::queue::Wait;

/// An ephemeral loopback port, which the operating system chooses.
fn ephemeral() -> SocketAddr {
    "127.0.0.1:0".parse().expect("a loopback address")
}

/// Binds a router on every address given, without serving it.
///
/// Separate from `listening` because half of these tests are about what
/// binding refuses, and a refusal has nothing to serve.
fn bound(addresses: &[SocketAddr], root: &ModelsRoot) -> Result<Router, Failure> {
    let catalog = Catalog::parse(&catalog_text("")).expect("a usable catalog");
    let server =
        Server::located(Some(&stub_binary())).expect("the stub binary is built by cargo test");
    let limits = Limits::new(
        Budget::new(None),
        IdleWindow::new(Duration::ZERO),
        Wait::new(Duration::ZERO),
    );
    Router::bind(
        addresses,
        Source {
            catalog,
            // These tests are about what binding accepts and refuses; none of
            // them reloads, so the path only has to exist as a value.
            path: root.path().join("catalog.toml"),
        },
        root.path().to_path_buf(),
        server,
        limits,
    )
}

/// The same, serving on a thread of its own.
///
/// Detached on purpose: `serve` never returns, so there is nothing to join.
/// The caller ends the children with `stop`.
fn listening(addresses: &[SocketAddr], root: &ModelsRoot) -> Arc<Router> {
    let router = Arc::new(bound(addresses, root).expect("ephemeral loopback ports"));
    let serving = Arc::clone(&router);
    thread::spawn(move || serving.serve());
    router
}

#[test]
fn every_address_bound_is_answered_by_the_one_router() {
    let root = ModelsRoot::with(&[MODEL]);
    let router = listening(&[ephemeral(), ephemeral()], &root);

    let addresses = router.addresses();
    assert_eq!(
        addresses.len(),
        2,
        "both addresses were asked for and both were bound: {addresses:?}"
    );
    assert_ne!(
        addresses[0], addresses[1],
        "two listeners, not one reported twice: {addresses:?}"
    );

    let first = request(addresses[0], &get("/models/gemma3/v1/echo"));
    let after_first = router.loaded();
    let second = request(addresses[1], &get("/models/gemma3/v1/echo"));
    let after_second = router.loaded();
    router.stop();

    assert_eq!(
        status(&first),
        Some(200),
        "the first address answers:\n{first}"
    );
    assert_eq!(
        status(&second),
        Some(200),
        "and so does the second:\n{second}"
    );
    assert_eq!(
        after_first,
        vec!["gemma3".to_string()],
        "the first request started the one child"
    );
    assert_eq!(
        after_second, after_first,
        "and the second address found that child rather than starting its own, \
         which is what makes this one router and not two"
    );
}

#[test]
fn the_listing_is_the_same_catalog_whichever_address_asks() {
    let root = ModelsRoot::with(&[MODEL]);
    let router = listening(&[ephemeral(), ephemeral()], &root);
    let addresses = router.addresses();

    let first = request(addresses[0], &get("/v1/models"));
    let second = request(addresses[1], &get("/v1/models"));
    router.stop();

    assert!(
        first.contains("\"gemma3\""),
        "the first address lists the catalog:\n{first}"
    );
    assert_eq!(
        body_of(&first),
        body_of(&second),
        "and the second lists the same one, because there is only one"
    );
}

/// What a reply carried after its head, which is what two listings are
/// compared on: the heads carry a date and would differ for that alone.
fn body_of(reply: &str) -> &str {
    reply.split_once("\r\n\r\n").map_or(reply, |(_, body)| body)
}

#[test]
fn a_wildcard_address_is_refused_because_it_names_no_interface() {
    let root = ModelsRoot::with(&[MODEL]);

    let refusal = bound(&["0.0.0.0:0".parse().expect("an address")], &root)
        .expect_err("a wildcard is refused")
        .to_string();

    assert!(
        refusal.contains("0.0.0.0:0"),
        "the refusal names what was asked for: {refusal}"
    );
    assert!(
        refusal.contains("interface"),
        "and says why a wildcard in particular is refused: {refusal}"
    );
}

#[test]
fn an_address_no_interface_holds_yet_is_waited_for_as_long_as_it_is_given() {
    // TEST-NET-1 is on no interface here, and stands in for a bridge that
    // comes up a moment after the router at boot. The router waits for it
    // rather than failing at once and leaving a service manager to retry --
    // which gives up after five restarts in ten seconds, taking loopback down
    // with the bridge.
    let (waited, said) = awaited(
        vec!["192.0.2.1:0".parse().expect("an address")],
        Duration::from_millis(400),
    );
    assert!(
        waited >= Duration::from_millis(400),
        "the address was waited for, as long as it was given; waited {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(5),
        "and no longer, so a router whose address never comes still says so: {waited:?}"
    );
    assert_eq!(
        said.matches("waiting for 192.0.2.1:0 to be assigned")
            .count(),
        1,
        "and said what it was waiting for, once rather than at every attempt: {said:?}"
    );
}

#[test]
fn an_address_that_can_be_bound_is_not_waited_for() {
    let (waited, said) = awaited(vec![ephemeral()], Duration::from_secs(30));
    assert!(
        waited < Duration::from_secs(5),
        "a loopback address binds at once, and startup does not wait on it"
    );
    assert!(said.is_empty(), "nor says it waited: {said:?}");
}

#[test]
fn an_address_refused_for_another_reason_is_not_waited_for() {
    // Only an address no interface holds is worth waiting for. A port that
    // is taken does not come free by itself, and is the bind's to report.
    let taken = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let within = Duration::from_secs(2);
    let (waited, said) = awaited(vec![taken.local_addr().expect("its address")], within);
    assert!(
        waited < within,
        "a port already taken was not waited for; waited {waited:?}"
    );
    assert!(said.is_empty(), "nor said to be: {said:?}");
}

/// Waits for `addresses` on a thread of its own, and gives back how long that
/// took and what it said -- failing, rather than hanging, if it never ends.
fn awaited(addresses: Vec<SocketAddr>, within: Duration) -> (Duration, String) {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let mut notices = Vec::new();
        let started = Instant::now();
        await_assigned(&addresses, within, &mut notices);
        let said = String::from_utf8_lossy(&notices).into_owned();
        drop(done.send((started.elapsed(), said)));
    });
    finished
        .recv_timeout(within + Duration::from_secs(10))
        .expect("the wait ended once it had waited as long as it was given")
}

#[test]
fn an_address_this_router_does_not_hold_is_the_systems_refusal_not_its_own() {
    let root = ModelsRoot::with(&[MODEL]);

    // TEST-NET-1: reserved for documentation, so it is on no interface here.
    // Binding it fails, and the point is which layer refuses. A machine that
    // permits non-local binds succeeds instead, which asserts the same thing.
    if let Err(failure) = bound(&["192.0.2.1:0".parse().expect("an address")], &root) {
        let refusal = failure.to_string();
        assert!(
            refusal.contains("cannot bind"),
            "a named address is refused by the operating system or not at all, \
             never by a rule of this router's: {refusal}"
        );
    }
}

#[test]
fn a_router_with_no_address_is_refused_rather_than_bound_to_nothing() {
    let root = ModelsRoot::with(&[MODEL]);

    let refusal = bound(&[], &root)
        .expect_err("nothing to listen on is not a router")
        .to_string();

    assert!(
        refusal.contains("address"),
        "the refusal says what was missing: {refusal}"
    );
}
