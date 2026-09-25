//! The generic endpoint, `/v1/...`, which names its model in the body, and
//! the listing beside it.
//!
//! The router reads this body to learn which child answers, and then forwards
//! the caller's own bytes rather than what it parsed.

use crate::support::{MODEL, ModelsRoot, catalog_text, get, post, request, serving, status};

/// A second model file, so routing between two entries is observable.
const SECOND_MODEL: &str = "cache/qwen/qwen3-8b.gguf";

/// A catalog carrying two entries, which is the smallest catalog in which
/// "the request reached the right child" can mean anything at all.
fn two_entries() -> String {
    catalog_text(&format!("\n[models.qwen38]\npath = \"{SECOND_MODEL}\"\n"))
}

#[test]
fn the_body_decides_which_child_answers() {
    let serving = serving(&two_entries(), ModelsRoot::with(&[MODEL, SECOND_MODEL]));

    for wanted in ["gemma3", "qwen38"] {
        let body = format!("{{\"model\":\"{wanted}\"}}");
        let reply = request(serving.address(), &post("/v1/echo", &body));

        assert_eq!(status(&reply), Some(200), "a child answered:\n{reply}");
        assert!(
            reply.contains(&format!("alias: {wanted}")),
            "the child started for '{wanted}' is the one that answered, \
             observed at the child rather than assumed at the router:\n{reply}"
        );
        assert!(
            reply.contains(&format!("body: {body}")),
            "the child received the caller's own bytes. The router read this \
             body to learn which model answers, and forwarding what it parsed \
             rather than what it was given would change a request it does not \
             own:\n{reply}"
        );
    }
}

#[test]
fn a_reasoning_effort_field_reaches_the_child_exactly_as_sent() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    // Unlike the single-field bodies beside this test, the spacing and field
    // order here change if the router forwards re-serialised JSON.
    let body = r#"{"reasoning_effort": "high", "model": "gemma3"}"#;
    let reply = request(serving.address(), &post("/v1/echo", body));

    assert_eq!(status(&reply), Some(200), "the child answered:\n{reply}");
    assert!(
        reply.contains(&format!("body: {body}")),
        "per-request reasoning effort must reach llama-server as the caller \
         wrote it, not as JSON re-serialised after routing:\n{reply}"
    );
}

#[test]
fn the_generic_endpoint_strips_no_prefix_from_the_path() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        &post("/v1/echo", "{\"model\":\"gemma3\"}"),
    );

    assert!(
        reply.contains("POST /v1/echo HTTP/1.1"),
        "the caller's own path is what the child is asked for, because the \
         model was named in the body rather than taken out of the path:\n{reply}"
    );
}

#[test]
fn a_body_naming_no_such_model_is_not_found_and_names_the_catalog() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        &post("/v1/chat/completions", "{\"model\":\"nowhere\"}"),
    );

    assert_eq!(status(&reply), Some(404), "no such entry:\n{reply}");
    assert!(
        reply.contains("nowhere") && reply.contains("gemma3"),
        "named the same way the dedicated endpoint names it:\n{reply}"
    );
}

#[test]
fn a_body_naming_no_model_at_all_is_a_bad_request() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        &post("/v1/chat/completions", "{\"messages\":[]}"),
    );

    assert_eq!(
        status(&reply),
        Some(400),
        "the generic endpoint has nothing else to route on:\n{reply}"
    );
    assert!(
        reply.contains("/models/"),
        "and says which endpoint needs no such field:\n{reply}"
    );
}

#[test]
fn a_generic_request_with_no_declared_body_is_told_what_is_missing() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    // What an OpenAI-compatible client sends to retrieve one model. It is not
    // the listing -- that is `/v1/models` exactly -- so it arrives here as a
    // generic request with nothing to route on.
    let reply = request(serving.address(), &get("/v1/models/gemma3"));

    assert_eq!(
        status(&reply),
        Some(411),
        "the missing thing is a declared body, and saying so is not the same \
         as a parser complaining about an empty one:\n{reply}"
    );
    assert!(
        reply.contains("Content-Length") && reply.contains("/models/"),
        "the refusal names the header it wanted and the endpoint that needs \
         no body at all:\n{reply}"
    );
}

#[test]
fn the_listing_answers_from_the_catalog_without_starting_anything() {
    // A models root with no files in it: starting any child would fail on the
    // missing model, so a 200 here is proof that none was attempted.
    let serving = serving(&two_entries(), ModelsRoot::with(&[]));

    let reply = request(serving.address(), &get("/v1/models"));

    assert_eq!(
        status(&reply),
        Some(200),
        "the router answers this:\n{reply}"
    );
    assert!(
        reply.contains("gemma3") && reply.contains("qwen38"),
        "every entry the catalog carries:\n{reply}"
    );
    assert!(
        reply.contains("\"object\":\"list\""),
        "in the shape an OpenAI-compatible client expects:\n{reply}"
    );
}
