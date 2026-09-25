//! A caller that makes no progress is given up on.
//!
//! The watch sees a caller leave. It cannot see one that stays and does
//! nothing -- sends none of its request, or reads none of its answer -- and
//! each of those held one of the router's threads, and the second a model, for
//! as long as it stayed. Both now have as long as the router's stall allows,
//! and no longer.

#![cfg(test)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

mod support;
use support::{MODEL, ModelsRoot, catalog_text, get, impatient, post, request, settled, status};

/// How long these routers let a caller make no progress: short, so a test
/// that waits for it does not spend the minute a router in service gives.
const STALL: Duration = Duration::from_millis(300);

#[test]
fn a_caller_that_sends_nothing_is_told_it_took_too_long() {
    let serving = impatient(&catalog_text(""), ModelsRoot::with(&[MODEL]), None, STALL);
    let mut idle = TcpStream::connect(serving.address()).expect("the router accepts");
    idle.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout, so a router that holds on fails rather than hangs");

    let started = Instant::now();
    let mut reply = String::new();
    let ended = idle.read_to_string(&mut reply);
    let waited = started.elapsed();

    assert!(
        ended.is_ok(),
        "the router ended the connection rather than holding it: {ended:?}"
    );
    assert_eq!(
        status(&reply),
        Some(408),
        "and said why, after waiting {waited:?}:\n{reply}"
    );
}

#[test]
fn a_caller_that_reads_nothing_of_its_answer_lets_the_model_go() {
    // Room for one model. The first streams far more than any pair of socket
    // buffers holds to a caller that reads none of it; once a write to that
    // caller has made no progress for the stall, the relay ends and the
    // second model can take the room.
    let catalog = format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         context_size = 4096\n\
         residency = \"on-demand\"\n\
         memory_estimate_mib = 512\n\
         startup_timeout_seconds = 30\n\
         \n\
         [models.chatty]\n\
         path = \"{MODEL}\"\n\
         [models.chatty.flags]\n\
         stream-events = \"5000000\"\n\
         \n\
         [models.other]\n\
         path = \"{MODEL}\"\n"
    );
    let serving = impatient(&catalog, ModelsRoot::with(&[MODEL]), Some(600), STALL);

    let mut caller = TcpStream::connect(serving.address()).expect("the router accepts");
    caller
        .write_all(post("/models/chatty/v1/chat/completions", "{}").as_bytes())
        .expect("the request is sent");
    settled(&serving, "the chatty model to load", |serving| {
        serving.loaded().contains(&"chatty".to_owned())
    });

    let started = Instant::now();
    let mut reply = String::new();
    while started.elapsed() < Duration::from_secs(10) {
        reply = request(serving.address(), &get("/models/other/v1/echo"));
        if status(&reply) == Some(200) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        status(&reply),
        Some(200),
        "a caller that read nothing for the stall let its model go, so the \
         other could take the room:\n{reply}"
    );
    drop(caller);
}
