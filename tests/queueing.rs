//! Waiting for room rather than refusing it.
//!
//! Its own target, beside `eviction.rs` rather than inside it: those cases
//! assert what a *full* router refuses, and every one of them would be held
//! for the length of a wait before asserting the same thing. Here the wait is
//! the subject.
//!
//! The distinction under test is between two refusals that look alike. A model
//! larger than the whole budget will never fit and is refused at once, however
//! long anybody waits. A model whose room is held by something still answering
//! will fit the moment that answer ends, and waiting for it is the difference
//! between a router that queues and one that makes every caller write a retry
//! loop.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

mod support;
use support::{MODEL, ModelsRoot, post, queued, request, status};

/// A second model file, so there is something to want the room for.
const SECOND_MODEL: &str = "cache/qwen/qwen3-8b.gguf";

/// Two entries that cannot both be held under the budget each test states.
///
/// The first is paced: twelve events a hundred milliseconds apart is over a
/// second of stream, which is long enough for a second request to be sent,
/// decided and answered while the first is still arriving -- on a loaded
/// continuous-integration machine as well as an idle one. The pacing costs
/// nothing in the case that never asks for a stream.
fn two_entries(first_mib: u32, second_mib: u32) -> String {
    format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         context_size = 4096\n\
         residency = \"on-demand\"\n\
         startup_timeout_seconds = 30\n\
         \n\
         [models.gemma3]\n\
         path = \"{MODEL}\"\n\
         memory_estimate_mib = {first_mib}\n\
         \n\
         [models.gemma3.flags]\n\
         stream-events = \"12\"\n\
         stream-gap = \"100\"\n\
         \n\
         [models.qwen38]\n\
         path = \"{SECOND_MODEL}\"\n\
         memory_estimate_mib = {second_mib}\n"
    )
}

/// The body that asks the stub for a paced stream.
const STREAM: &str = "{\"model\":\"gemma3\",\"stream\":true}";

/// Opens a streamed request and returns once the first bytes have arrived.
///
/// Waited on rather than assumed: the child has to be started and the reply
/// has to have begun before the model counts as busy, or the test races the
/// thing it is asserting about.
fn start_stream(address: SocketAddr, path: &str, body: &str) -> TcpStream {
    let mut stream = TcpStream::connect(address).expect("the router is listening");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a read timeout, so a hang fails rather than blocking the suite");
    stream
        .write_all(post(path, body).as_bytes())
        .expect("write");

    let mut first = [0u8; 512];
    let read = stream.read(&mut first).expect("the router answers");
    assert!(read > 0, "the stream began before anything else happened");
    stream
}

#[test]
fn a_request_waits_for_room_a_busy_model_is_holding() {
    // 3000 and 3000 against 4096: whichever is loaded, the other needs its
    // room. The first is made busy, so there is nothing to evict until its
    // answer finishes.
    let serving = queued(
        &two_entries(3000, 3000),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );

    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);

    // Asked for while the first is still answering. Before the wait existed
    // this was a 503 the instant it arrived.
    let began = Instant::now();
    let second = request(
        serving.address(),
        &post("/models/qwen38/v1/echo", "{\"say\":\"hello\"}"),
    );
    let waited = began.elapsed();

    assert_eq!(
        status(&second),
        Some(200),
        "the room freed up when the first answer finished, and the second \
         request was still there to take it:\n{second}"
    );
    assert!(
        waited >= Duration::from_millis(200),
        "it was answered in {waited:?}, which is too fast to have waited for \
         a stream that takes over a second: the room cannot have been held"
    );

    drop(streaming);
}

#[test]
fn a_wait_of_zero_refuses_exactly_as_it_did_before() {
    let serving = queued(
        &two_entries(3000, 3000),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::ZERO,
    );

    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);

    let began = Instant::now();
    let second = request(
        serving.address(),
        &post("/models/qwen38/v1/echo", "{\"say\":\"hello\"}"),
    );
    let waited = began.elapsed();

    assert_eq!(
        status(&second),
        Some(503),
        "an operator who set 0 said 'do not wait', and gets the refusal this \
         router gave before waiting existed:\n{second}"
    );
    assert!(
        waited < Duration::from_secs(1),
        "refused in {waited:?}, which is long enough to have waited: zero has \
         to mean zero or the setting says nothing"
    );
    assert!(
        second.contains("gemma3"),
        "the refusal names what is holding the memory:\n{second}"
    );

    drop(streaming);
}

/// Three large entries and a small one under a budget of 4096 MiB: any large
/// one fills most of it, and the small one fits beside any of them. The first
/// streams for three seconds, long enough to ask for the others while it does.
fn contended() -> String {
    format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         context_size = 4096\n\
         residency = \"on-demand\"\n\
         startup_timeout_seconds = 30\n\
         memory_estimate_mib = 3000\n\
         \n\
         [models.gemma3]\n\
         path = \"{MODEL}\"\n\
         [models.gemma3.flags]\n\
         stream-events = \"30\"\n\
         stream-gap = \"100\"\n\
         \n\
         [models.qwen38]\n\
         path = \"{SECOND_MODEL}\"\n\
         \n\
         [models.third]\n\
         path = \"{SECOND_MODEL}\"\n\
         \n\
         [models.tiny]\n\
         path = \"{MODEL}\"\n\
         memory_estimate_mib = 512\n"
    )
}

/// How long a request sent a moment ago is given to be waiting in line before
/// the test does the next thing -- long enough on a loaded machine as well as
/// an idle one, and well inside the three seconds the stream holds the room.
const IN_LINE: Duration = Duration::from_secs(1);

/// Asks for `id` on a thread of its own, and sends back its reply once it has
/// one, so a test can see which of several waiting requests finished first.
fn asked(
    address: SocketAddr,
    id: &'static str,
    replies: &std::sync::mpsc::Sender<(&'static str, String)>,
) {
    let replies = replies.clone();
    std::thread::spawn(move || {
        let reply = request(
            address,
            &post(&format!("/models/{id}/v1/echo"), "{\"say\":\"hello\"}"),
        );
        replies.send((id, reply)).ok();
    });
}

#[test]
fn a_model_that_fits_is_answered_while_another_request_waits_for_room() {
    // Waiting for room used to hold the lock every admission takes, so a
    // model with room to spare queued behind a request waiting for somebody
    // else's -- for as long as that request waited.
    let serving = queued(
        &contended(),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );
    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    std::thread::sleep(Duration::from_millis(200));
    asked(serving.address(), "tiny", &replies);

    let (first, reply) = answered
        .recv_timeout(Duration::from_secs(20))
        .expect("a request was answered");
    assert_eq!(
        (first, status(&reply)),
        ("tiny", Some(200)),
        "the model that fits was answered while qwen38 still waited for \
         gemma3's stream to end:\n{reply}"
    );

    let (second, reply) = answered
        .recv_timeout(Duration::from_secs(10))
        .expect("the waiting request was answered once the room freed up");
    assert_eq!((second, status(&reply)), ("qwen38", Some(200)), "{reply}");
    drop(streaming);
}

#[test]
fn requests_waiting_for_room_are_answered_in_the_order_they_asked() {
    let serving = queued(
        &contended(),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );
    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    std::thread::sleep(IN_LINE);
    asked(serving.address(), "third", &replies);

    let order: Vec<_> = (0..2)
        .map(|_| {
            let (id, reply) = answered
                .recv_timeout(Duration::from_secs(20))
                .expect("every waiting request was answered");
            assert_eq!(status(&reply), Some(200), "{id}:\n{reply}");
            id
        })
        .collect();
    assert_eq!(
        order,
        ["qwen38", "third"],
        "the request that asked first took the room first"
    );
    drop(streaming);
}

#[test]
fn a_request_waiting_when_the_catalog_is_reloaded_is_told_to_ask_again() {
    // It waited under the catalog it arrived with. Deciding under that one
    // after another had replaced it would admit an entry by one set of
    // numbers and insert it into the slots of another, so it is told to ask
    // again -- which a retry does under the catalog now serving.
    let root = ModelsRoot::with(&[MODEL, SECOND_MODEL]);
    // Where every router `support` builds reads its catalog when reloaded.
    std::fs::write(root.path().join("catalog.toml"), contended())
        .expect("a writable temporary directory");
    let serving = queued(&contended(), root, Some(4096), Duration::from_secs(30));
    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    std::thread::sleep(IN_LINE);

    let reloaded = request(serving.address(), &post("/reload", ""));
    assert_eq!(status(&reloaded), Some(200), "{reloaded}");

    let (_, reply) = answered
        .recv_timeout(Duration::from_secs(20))
        .expect("the waiting request was answered");
    assert_eq!(status(&reply), Some(503), "{reply}");
    assert!(
        reply.contains("reloaded"),
        "and told why, so that asking again is the obvious next step:\n{reply}"
    );
    drop(streaming);
}

#[test]
fn a_model_larger_than_the_budget_is_refused_without_waiting() {
    // 5000 against 4096: no eviction makes this fit, so waiting for one would
    // be waiting for something that cannot happen.
    let serving = queued(
        &two_entries(1000, 5000),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );

    let began = Instant::now();
    let reply = request(
        serving.address(),
        &post("/models/qwen38/v1/echo", "{\"say\":\"hello\"}"),
    );
    let waited = began.elapsed();

    assert_eq!(
        status(&reply),
        Some(503),
        "nothing can be unloaded to make it fit:\n{reply}"
    );
    assert!(
        waited < Duration::from_secs(1),
        "refused in {waited:?}: a permanent refusal must not be held for the \
         wait, because no amount of waiting changes the answer"
    );
}
