//! What a caller sees of the child behind its request: one that cannot
//! start, one that never becomes ready, a reply that ends when the router is
//! done with it, and children that end with the router that started them.

use std::time::{Duration, Instant};

use crate::support::poll::eventually;
use crate::support::{MODEL, ModelsRoot, catalog_text, get, health, request, serving, status};

#[test]
fn a_relayed_reply_ends_when_the_router_is_done_with_it() {
    // On Windows a cloned socket is a handle a child spawned while it is open
    // inherits (rust-lang/rust#70719). A request that loaded a model held a
    // clone of its caller's socket as the child started, so its connection
    // stayed open until that model exited: a caller reading to the end waited
    // out its own timeout, every time, for every relayed reply.
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let started = Instant::now();
    let reply = request(serving.address(), &get("/models/gemma3/v1/echo"));
    let took = started.elapsed();

    assert_eq!(status(&reply), Some(200), "{reply}");
    assert!(
        took < Duration::from_secs(10),
        "the reply ended when the router was done with it, not when the \
         caller gave up reading; it took {took:?}"
    );
}

#[test]
fn an_entry_whose_child_never_becomes_ready_is_a_gateway_timeout() {
    let serving = serving(
        &catalog_text(
            "startup_timeout_seconds = 2\n\
             \n\
             [models.gemma3.flags]\n\
             ready-after = \"600000\"\n",
        ),
        ModelsRoot::with(&[MODEL]),
    );

    let reply = request(serving.address(), &get("/models/gemma3/v1/echo"));

    assert_eq!(status(&reply), Some(504), "the budget ran out:\n{reply}");
    assert!(
        reply.contains("gemma3"),
        "naming the entry, so the reader is not sent to the catalog to \
         guess:\n{reply}"
    );
}

#[test]
fn an_entry_whose_child_cannot_start_is_a_bad_gateway() {
    // A root with no files in it: the model the entry names is not there, and
    // slice 2 refuses before it spawns anything.
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[]));

    let reply = request(serving.address(), &get("/models/gemma3/v1/echo"));

    assert_eq!(
        status(&reply),
        Some(502),
        "the child could not be started:\n{reply}"
    );
    assert!(reply.contains("gemma3"), "naming the entry:\n{reply}");
}

#[test]
fn stopping_a_router_ends_the_children_it_started() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));
    let reply = request(serving.address(), &get("/models/gemma3/v1/echo"));

    // The stub reflects the Host it was given, which the rewrite set to the
    // child's own address. That is how this test learns a port the router
    // never told anyone about.
    let endpoint = reply
        .lines()
        .find_map(|line| line.strip_prefix("Host: "))
        .expect("the echo carries the address the child was reached on")
        .to_owned();
    assert_eq!(
        health(endpoint.as_str()),
        Some(200),
        "the child answers while the router holds it"
    );

    drop(serving);

    // Polled rather than slept on: killing a process is not instantaneous and
    // a fixed wait would be either flaky or slow.
    assert!(
        eventually(Duration::from_secs(10), Duration::from_millis(50), || {
            health(endpoint.as_str()).is_none()
        }),
        "the child at {endpoint} outlived the router that started it. A child \
         is a separate process, and nothing in the operating system ends it \
         when this one goes."
    );
}
