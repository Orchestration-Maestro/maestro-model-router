//! Loading a model only into free room, when a request asks.
//!
//! Its own module beside `eviction_policy.rs`, which proves what the router
//! unloads to make room: these prove that one request can forbid it. A
//! caller whose request must never cost another model its place -- a search
//! embedding a query while somebody is talking to a chat model -- sends
//! `X-Model-Router-Room: free`, and is refused rather than served by an
//! unload.
//!
//! Every case states its budget directly and its estimates in the catalog
//! text, for the reasons `eviction_policy.rs` gives.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::support::{MODEL, ModelsRoot, Serving, budgeted, get, post, queued, request, status};

/// A second model file, so there is something to want the room for.
const SECOND_MODEL: &str = "cache/qwen/qwen3-8b.gguf";

/// What a caller sends to keep its request to free room.
const FREE_ROOM: &str = "X-Model-Router-Room: free";

/// Two entries, each stating what it is estimated to cost.
///
/// gemma3 streams for three seconds when it is asked to, which is long enough
/// to ask for qwen38 while it does -- and long enough that a request held
/// until the stream ends is told apart from one refused at once.
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
         stream-events = \"30\"\n\
         stream-gap = \"100\"\n\
         \n\
         [models.qwen38]\n\
         path = \"{SECOND_MODEL}\"\n\
         memory_estimate_mib = {second_mib}\n"
    )
}

/// The same request, carrying one more header after its request line.
fn with_header(raw: &str, header: &str) -> String {
    raw.replacen("\r\n", &format!("\r\n{header}\r\n"), 1)
}

/// A router whose budget gemma3 fills, with gemma3 loaded and idle.
///
/// 3000 and 3000 against 4096: qwen38 can load only if gemma3 is unloaded,
/// and gemma3 is a candidate, so without the header it goes.
fn one_idle_model_filling_the_budget() -> Serving {
    let serving = budgeted(
        &two_entries(3000, 3000),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
    );
    let warm = request(serving.address(), &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&warm), Some(200), "gemma3 is loaded:\n{warm}");
    serving
}

#[test]
fn a_request_for_free_room_is_refused_rather_than_unload_an_idle_model() {
    let serving = one_idle_model_filling_the_budget();

    let reply = request(
        serving.address(),
        &with_header(&get("/models/qwen38/v1/echo"), FREE_ROOM),
    );

    assert_eq!(
        status(&reply),
        Some(503),
        "the only room is gemma3's, and the caller forbade taking it:\n{reply}"
    );
    assert!(
        reply.contains("\"code\":\"insufficient_room\""),
        "the code a program switches on:\n{reply}"
    );
    assert!(
        reply.contains("X-Model-Router-Room") && reply.contains("gemma3"),
        "the message names the header and the model that would have gone:\n{reply}"
    );
    assert_eq!(
        serving.loaded(),
        vec!["gemma3".to_owned()],
        "and nothing was unloaded"
    );
}

#[test]
fn without_the_header_the_same_request_unloads_the_idle_model() {
    let serving = one_idle_model_filling_the_budget();

    let reply = request(serving.address(), &get("/models/qwen38/v1/echo"));

    assert_eq!(
        status(&reply),
        Some(200),
        "qwen38 is served, having made its own room:\n{reply}"
    );
    assert_eq!(
        serving.loaded(),
        vec!["qwen38".to_owned()],
        "by unloading gemma3, as a request with no header always has"
    );
}

#[test]
fn a_model_already_loaded_is_served_to_a_request_for_free_room() {
    let serving = one_idle_model_filling_the_budget();

    let reply = request(
        serving.address(),
        &with_header(&get("/models/gemma3/v1/echo"), FREE_ROOM),
    );

    assert_eq!(
        status(&reply),
        Some(200),
        "the budget is full, and serving what holds it costs no room:\n{reply}"
    );
    assert_eq!(serving.loaded(), vec!["gemma3".to_owned()]);
}

#[test]
fn a_model_that_fits_beside_the_loaded_one_loads_in_free_room() {
    // 512 and 512 against 4096: qwen38 fits without anything going.
    let serving = budgeted(
        &two_entries(512, 512),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
    );
    let warm = request(serving.address(), &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&warm), Some(200), "gemma3 is loaded:\n{warm}");

    let reply = request(
        serving.address(),
        &with_header(&get("/models/qwen38/v1/echo"), FREE_ROOM),
    );

    assert_eq!(status(&reply), Some(200), "free room is room:\n{reply}");
    assert_eq!(
        serving.loaded(),
        vec!["gemma3".to_owned(), "qwen38".to_owned()],
        "both are held, because nothing had to go"
    );
}

#[test]
fn the_generic_endpoint_and_a_load_keep_to_free_room_too() {
    let serving = one_idle_model_filling_the_budget();

    // The generic endpoint routes on the body, and a load names its model
    // there too: neither reads the model from the path, which is the one
    // way of asking the dedicated endpoint's case does not cover.
    for path in ["/v1/echo", "/models/load"] {
        let reply = request(
            serving.address(),
            &with_header(&post(path, "{\"model\":\"qwen38\"}"), FREE_ROOM),
        );
        assert_eq!(status(&reply), Some(503), "{path}:\n{reply}");
        assert!(
            reply.contains("\"code\":\"insufficient_room\""),
            "{path}:\n{reply}"
        );
    }
    assert_eq!(
        serving.loaded(),
        vec!["gemma3".to_owned()],
        "neither unloaded gemma3"
    );
}

#[test]
fn a_request_for_free_room_is_refused_at_once_rather_than_wait_for_a_busy_model() {
    // gemma3 fills the budget and is answering, so the only room qwen38 could
    // have is gemma3's once that answer ends. Without the header the request
    // would wait for it and then unload gemma3. With it, the room never
    // frees on its own -- a model that stops being busy is still loaded --
    // so waiting could only end in the same refusal, later.
    let serving = queued(
        &two_entries(3000, 3000),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Duration::from_secs(30),
    );
    let mut streaming = TcpStream::connect(serving.address()).expect("the router is listening");
    streaming
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a read timeout, so a hang fails rather than blocking the suite");
    streaming
        .write_all(post("/models/gemma3/v1/chat/completions", "{}").as_bytes())
        .expect("the stream is asked for");
    let mut first = [0u8; 1];
    assert_eq!(
        streaming.read(&mut first).expect("the stream began"),
        1,
        "gemma3 is answering before qwen38 is asked for"
    );

    let began = Instant::now();
    let reply = request(
        serving.address(),
        &with_header(&get("/models/qwen38/v1/echo"), FREE_ROOM),
    );
    let waited = began.elapsed();

    assert_eq!(status(&reply), Some(503), "{reply}");
    assert!(
        reply.contains("\"code\":\"insufficient_room\""),
        "refused for want of room, not told to retry:\n{reply}"
    );
    assert!(
        waited < Duration::from_secs(1),
        "refused in {waited:?}, which is long enough to have waited for a \
         stream that takes three seconds"
    );
    assert_eq!(serving.loaded(), vec!["gemma3".to_owned()]);
}

#[test]
fn a_room_other_than_free_is_refused_rather_than_ignored() {
    let serving = budgeted(
        &two_entries(512, 512),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
    );

    // Read as no header, either of these would let the request unload the
    // model its caller meant to keep.
    for value in ["any", ""] {
        let reply = request(
            serving.address(),
            &with_header(
                &get("/models/gemma3/v1/echo"),
                &format!("X-Model-Router-Room: {value}"),
            ),
        );
        assert_eq!(status(&reply), Some(400), "'{value}':\n{reply}");
        assert!(
            reply.contains("\"code\":\"unknown_room\""),
            "'{value}':\n{reply}"
        );
    }
    assert!(
        serving.loaded().is_empty(),
        "nothing was loaded for a request whose terms were not understood: {:?}",
        serving.loaded()
    );
}
