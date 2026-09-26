//! Loading a model only into free room, when a request asks.
//!
//! Its own module beside `eviction_policy.rs`, which proves what the router
//! unloads to make room: these prove that one request can forbid it. A
//! caller whose request must never cost another model its place -- a search
//! embedding a query while somebody is talking to a chat model -- sends
//! `X-Model-Router-Room: free`, and is refused rather than served by an
//! unload. What such a request loads is a guest, and gives its room back
//! before any other model when a later request needs it.
//!
//! Every case states its budget directly and its estimates in the catalog
//! text, for the reasons `eviction_policy.rs` gives.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use maestro_model_router::memory::{Fixed, Measurement, Probe};

use crate::support::poll::eventually;
use crate::support::{
    MODEL, ModelsRoot, Serving, budgeted, get, post, probed, queued, request, status,
};

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

#[test]
fn free_room_is_counted_from_what_a_loaded_model_holds_not_from_its_estimate() {
    // 512 and 512 against 4096 fit together on paper. Every child on this
    // machine measures 4000 MiB once loaded, so gemma3 really holds nearly
    // the whole budget and there is no free room beside it:
    // `eviction_policy.rs` makes this room by unloading gemma3, which this
    // request forbids.
    let serving = probed(
        &two_entries(512, 512),
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
        Probe::Fixed(Fixed {
            device: None,
            system_total_mib: None,
            measurement: Measurement {
                resident_mib: Some(4000),
                device_mib: None,
            },
        }),
    );
    let warm = request(serving.address(), &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&warm), Some(200), "gemma3 is loaded:\n{warm}");

    let reply = request(
        serving.address(),
        &with_header(&get("/models/qwen38/v1/echo"), FREE_ROOM),
    );

    assert_eq!(
        status(&reply),
        Some(503),
        "counted at its estimate, gemma3 would have left room for qwen38; \
         counted at what it holds, it leaves none:\n{reply}"
    );
    assert!(reply.contains("\"code\":\"insufficient_room\""), "{reply}");
    assert_eq!(serving.loaded(), vec!["gemma3".to_owned()]);
}

#[test]
fn a_child_that_exited_on_its_own_holds_no_free_room() {
    // gemma3 fills the budget and then exits by itself. With no idle window
    // nothing sweeps its slot, so the router still counts it -- and would
    // refuse this request for room held by a process that is gone.
    let catalog = two_entries(3000, 3000).replacen(
        "[models.gemma3.flags]\n",
        "[models.gemma3.flags]\nexit-after = \"2000\"\n",
        1,
    );
    let serving = budgeted(
        &catalog,
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
    );
    let warm = request(serving.address(), &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&warm), Some(200), "gemma3 is loaded:\n{warm}");

    // Asked until it is served, for as long as the child may take to exit.
    // Until it does, gemma3 really holds the room and a refusal is right; a
    // router that went on counting the dead child refuses every time. The
    // last reply is kept, so a failure shows what the router said.
    let mut reply = String::new();
    let served = eventually(Duration::from_secs(10), Duration::from_millis(100), || {
        reply = request(
            serving.address(),
            &with_header(&get("/models/qwen38/v1/echo"), FREE_ROOM),
        );
        status(&reply) == Some(200)
    });

    assert!(
        served,
        "gemma3's child is gone, and so is the room it held:\n{reply}"
    );
    assert_eq!(serving.loaded(), vec!["qwen38".to_owned()]);
}

#[test]
fn a_model_loaded_into_free_room_gives_its_room_back_before_a_chat_model() {
    // gemma3 and qwen38 are chat models, and the embedder is what a search
    // loads into free room. gemma3's 2000 and the embedder's 1000 fit in 4096
    // together; qwen38's 1500 fits beside either of them but not both, so
    // one has to go. gemma3 answered longest ago, which is what chose the
    // model to unload before guests existed.
    let catalog = format!(
        "{}\n\
         [models.embedder]\n\
         path = \"{MODEL}\"\n\
         memory_estimate_mib = 1000\n",
        two_entries(2000, 1500)
    );
    let serving = budgeted(
        &catalog,
        ModelsRoot::with(&[MODEL, SECOND_MODEL]),
        Some(4096),
    );
    let chat = request(serving.address(), &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&chat), Some(200), "gemma3 is loaded:\n{chat}");
    let search = request(
        serving.address(),
        &with_header(&get("/models/embedder/v1/echo"), FREE_ROOM),
    );
    assert_eq!(
        status(&search),
        Some(200),
        "the embedder is loaded into the free room beside gemma3:\n{search}"
    );

    let reply = request(serving.address(), &get("/models/qwen38/v1/echo"));

    assert_eq!(status(&reply), Some(200), "{reply}");
    assert_eq!(
        serving.loaded(),
        vec!["gemma3".to_owned(), "qwen38".to_owned()],
        "the embedder, a guest, gave its room back; gemma3 stayed though it \
         answered longest ago"
    );
}
