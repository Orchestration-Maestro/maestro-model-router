//! Waiting for room rather than refusing it.
//!
//! Its own module, beside `eviction_policy.rs` rather than inside it: those cases
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

use crate::support::{MODEL, ModelsRoot, post, queued, request, settled, status};

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

/// Three large entries and two small ones under a budget of 4096 MiB: any
/// large one fills most of it, and a small one fits beside it. gemma3 streams
/// for three seconds, long enough to ask for the others while it does; the
/// other large ones stream for one, so the order they are served in shows as
/// a second between their first bytes rather than as a race between threads.
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
         [models.qwen38.flags]\n\
         stream-events = \"10\"\n\
         stream-gap = \"100\"\n\
         \n\
         [models.third]\n\
         path = \"{SECOND_MODEL}\"\n\
         [models.third.flags]\n\
         stream-events = \"10\"\n\
         stream-gap = \"100\"\n\
         \n\
         [models.tiny]\n\
         path = \"{MODEL}\"\n\
         memory_estimate_mib = 512\n\
         \n\
         [models.small]\n\
         path = \"{SECOND_MODEL}\"\n\
         memory_estimate_mib = 1000\n"
    )
}

/// One reply to a request asked on a thread of its own.
struct Reply {
    id: &'static str,
    /// When its first byte arrived, which is when the router began serving
    /// it -- and, unlike when a thread got round to reporting it, a moment
    /// the router chose.
    began: Instant,
    text: String,
}

/// Asks `id` for a stream on a thread of its own, and sends back the reply
/// with when it began.
fn asked(address: SocketAddr, id: &'static str, replies: &std::sync::mpsc::Sender<Reply>) {
    let replies = replies.clone();
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).expect("the router is listening");
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("a read timeout, so a hang fails rather than blocking the suite");
        stream
            .write_all(post(&format!("/models/{id}/v1/chat/completions"), "{}").as_bytes())
            .expect("the request is sent");
        let mut first = [0u8; 1];
        let read = stream.read(&mut first).unwrap_or(0);
        let began = Instant::now();
        let mut text = String::from_utf8_lossy(&first[..read]).into_owned();
        drop(stream.read_to_string(&mut text));
        replies.send(Reply { id, began, text }).ok();
    });
}

/// The next `count` replies, each checked to be a success, by the request
/// they answered, in the order the router began serving them.
fn served_in_order(answered: &std::sync::mpsc::Receiver<Reply>, count: usize) -> Vec<&'static str> {
    let mut replies: Vec<Reply> = (0..count)
        .map(|_| {
            answered
                .recv_timeout(Duration::from_secs(30))
                .expect("every waiting request was answered")
        })
        .collect();
    for reply in &replies {
        assert_eq!(
            status(&reply.text),
            Some(200),
            "{}:\n{}",
            reply.id,
            reply.text
        );
    }
    replies.sort_by_key(|reply| reply.began);
    replies.iter().map(|reply| reply.id).collect()
}

/// A router over `contended`, with gemma3 streaming and so holding its room.
fn contended_and_streaming() -> (crate::support::Serving, TcpStream) {
    let serving = queued(
        &contended(),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );
    let streaming = start_stream(serving.address(), "/v1/chat/completions", STREAM);
    (serving, streaming)
}

/// Waits until this many requests are in line for room.
fn in_line(serving: &crate::support::Serving, count: usize) {
    settled(serving, &format!("{count} requests in line"), |serving| {
        serving.waiting() == count
    });
}

#[test]
fn a_model_that_fits_is_answered_while_another_request_waits_for_room() {
    // Waiting for room used to hold the lock every admission takes, so a
    // model with room to spare queued behind a request waiting for somebody
    // else's -- for as long as that request waited.
    let (serving, streaming) = contended_and_streaming();
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    in_line(&serving, 1);
    asked(serving.address(), "tiny", &replies);

    assert_eq!(
        served_in_order(&answered, 2),
        ["tiny", "qwen38"],
        "the model that fits was served while qwen38 still waited for \
         gemma3's stream to end"
    );
    drop(streaming);
}

#[test]
fn requests_waiting_for_room_are_answered_in_the_order_they_asked() {
    let (serving, streaming) = contended_and_streaming();
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    in_line(&serving, 1);
    asked(serving.address(), "third", &replies);
    in_line(&serving, 2);

    assert_eq!(
        served_in_order(&answered, 2),
        ["qwen38", "third"],
        "the request that asked first took the room first"
    );
    assert_eq!(serving.waiting(), 0, "and nobody is left in line");
    drop(streaming);
}

#[test]
fn a_request_that_could_take_room_now_waits_its_turn_behind_an_earlier_one() {
    // tiny is loaded and idle, and gemma3 streams beside it. qwen38 needs the
    // room gemma3 holds, and waits. small could have room at once, by
    // unloading tiny -- and joins the line instead, because qwen38 asked
    // first.
    let (serving, streaming) = contended_and_streaming();
    let warm = request(
        serving.address(),
        &post("/models/tiny/v1/echo", "{\"say\":\"hello\"}"),
    );
    assert_eq!(status(&warm), Some(200), "{warm}");
    let (replies, answered) = std::sync::mpsc::channel();
    asked(serving.address(), "qwen38", &replies);
    in_line(&serving, 1);
    asked(serving.address(), "small", &replies);

    in_line(&serving, 2);
    assert!(
        !serving.loaded().contains(&"small".to_owned()),
        "small took no room while qwen38 waited: {:?}",
        serving.loaded()
    );
    assert_eq!(served_in_order(&answered, 2).len(), 2, "both were served");
    assert_eq!(serving.waiting(), 0, "and nobody is left in line");
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
    in_line(&serving, 1);

    let reloaded = request(serving.address(), &post("/reload", ""));
    assert_eq!(status(&reloaded), Some(200), "{reloaded}");

    let reply = answered
        .recv_timeout(Duration::from_secs(20))
        .expect("the waiting request was answered");
    assert_eq!(status(&reply.text), Some(503), "{}", reply.text);
    assert!(
        reply.text.contains("reloaded"),
        "and told why, so that asking again is the obvious next step:\n{}",
        reply.text
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
