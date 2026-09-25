//! Loading and unloading a model on an operator's say-so.
//!
//! The router loads a model on the first request for it and lets it go when
//! it is idle, room is wanted, or the router stops. Neither waits for a person.
//! These two endpoints are for the times one is waiting: warming a model before
//! the request that would otherwise pay for the load, and giving memory back
//! now rather than after the idle window.
//!
//! The paths and the body are llama.cpp's own, from its router mode --
//! `POST /models/load` and `POST /models/unload`, each naming its model as
//! `{"model": ...}` -- so a llama.cpp client that offers those buttons works
//! against this router as it does against that one.

use std::io::Write;
use std::net::TcpStream;

use serde_json::Value;

use crate::support::{
    MODEL, ModelsRoot, budgeted, catalog_text, get, post, request, serving, settled, status,
};

/// The body both endpoints take, naming one model.
fn naming(model: &str) -> String {
    format!("{{\"model\":\"{model}\"}}")
}

/// The JSON body of a reply, or a panic naming what arrived instead.
fn body(reply: &str) -> Value {
    let body = reply.split_once("\r\n\r\n").map_or_else(
        || panic!("a reply with a body, got:\n{reply}"),
        |(_, body)| body,
    );
    serde_json::from_str(body).unwrap_or_else(|error| {
        panic!("a JSON body ({error}), got:\n{body}");
    })
}

#[test]
fn a_load_has_the_model_ready_before_it_replies() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(serving.address(), &post("/models/load", &naming("gemma3")));

    assert_eq!(status(&reply), Some(200), "got:\n{reply}");
    assert_eq!(body(&reply)["success"], true, "got:\n{reply}");
    // Checked at once rather than settled on: a load that replied before the
    // model was ready would leave the caller to find out by asking it.
    assert_eq!(serving.loaded(), vec!["gemma3".to_owned()]);
}

#[test]
fn an_unload_lets_an_idle_model_go() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));
    let warmed = request(serving.address(), &post("/models/load", &naming("gemma3")));
    assert_eq!(status(&warmed), Some(200), "got:\n{warmed}");

    let reply = request(
        serving.address(),
        &post("/models/unload", &naming("gemma3")),
    );

    assert_eq!(status(&reply), Some(200), "got:\n{reply}");
    assert_eq!(body(&reply)["success"], true, "got:\n{reply}");
    assert!(
        serving.loaded().is_empty(),
        "loaded: {:?}",
        serving.loaded()
    );
}

#[test]
fn an_unload_of_a_model_nothing_holds_is_already_done() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        &post("/models/unload", &naming("gemma3")),
    );

    assert_eq!(status(&reply), Some(200), "got:\n{reply}");
}

#[test]
fn a_model_the_catalog_does_not_carry_is_neither_loaded_nor_unloaded() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    for path in ["/models/load", "/models/unload"] {
        let reply = request(serving.address(), &post(path, &naming("absent")));
        assert_eq!(status(&reply), Some(404), "{path}, got:\n{reply}");
        assert!(reply.contains("model_not_found"), "{path}, got:\n{reply}");
    }
}

#[test]
fn a_model_that_is_answering_is_not_unloaded_from_under_its_caller() {
    let catalog = catalog_text("[models.gemma3.flags]\nfirst-byte-after = \"10000\"\n");
    let serving = serving(&catalog, ModelsRoot::with(&[MODEL]));
    let mut caller = TcpStream::connect(serving.address()).expect("the router accepts");
    caller
        .write_all(post("/models/gemma3/v1/chat/completions", "{}").as_bytes())
        .expect("the request is sent");
    settled(&serving, "the model to load", |serving| {
        serving.loaded().contains(&"gemma3".to_owned())
    });

    let reply = request(
        serving.address(),
        &post("/models/unload", &naming("gemma3")),
    );

    assert_eq!(status(&reply), Some(409), "got:\n{reply}");
    assert!(reply.contains("model_busy"), "got:\n{reply}");
    assert_eq!(serving.loaded(), vec!["gemma3".to_owned()]);
}

#[test]
fn a_load_there_is_no_room_for_is_refused_as_a_request_would_be() {
    let serving = budgeted(&catalog_text(""), ModelsRoot::with(&[MODEL]), Some(100));

    let reply = request(serving.address(), &post("/models/load", &naming("gemma3")));

    assert_eq!(status(&reply), Some(503), "got:\n{reply}");
    assert!(
        serving.loaded().is_empty(),
        "loaded: {:?}",
        serving.loaded()
    );
}

#[test]
fn only_a_post_loads_or_unloads() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    for path in ["/models/load", "/models/unload"] {
        let reply = request(serving.address(), &get(path));
        assert_eq!(status(&reply), Some(405), "{path}, got:\n{reply}");
        assert!(
            reply.contains("Allow: POST, OPTIONS"),
            "{path}, got:\n{reply}"
        );
    }
    assert!(
        serving.loaded().is_empty(),
        "loaded: {:?}",
        serving.loaded()
    );
}
