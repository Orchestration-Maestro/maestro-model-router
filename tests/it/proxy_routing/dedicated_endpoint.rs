//! The dedicated endpoint, `/models/<id>/...`, which names its model in the
//! path.
//!
//! The router strips the prefix, rewrites the head for the child and copies
//! the body through without reading it. What the child receives is observed
//! at the child, through the stub's echo, rather than assumed at the router.

use crate::support::{MODEL, ModelsRoot, catalog_text, get, post, request, serving, status};

#[test]
fn an_unknown_identifier_is_not_found_and_names_what_the_catalog_carries() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(serving.address(), &get("/models/nowhere/v1/models"));

    assert_eq!(status(&reply), Some(404), "no such entry:\n{reply}");
    assert!(
        reply.contains("nowhere"),
        "the refusal names what was asked for:\n{reply}"
    );
    assert!(
        reply.contains("gemma3"),
        "and what the catalog does carry, so the reader can correct it:\n{reply}"
    );
}

#[test]
fn a_path_that_is_no_shape_the_router_serves_is_refused() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(serving.address(), &get("/health"));

    assert_eq!(
        status(&reply),
        Some(404),
        "the router serves two shapes and no others:\n{reply}"
    );
}

#[test]
fn a_request_reaches_the_child_with_the_prefix_stripped() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(serving.address(), &get("/models/gemma3/v1/echo"));

    assert_eq!(status(&reply), Some(200), "the child answered:\n{reply}");
    assert!(
        reply.contains("GET /v1/echo HTTP/1.1"),
        "the child is asked for the path without the prefix, observed at the \
         child rather than assumed at the router:\n{reply}"
    );
    assert!(
        reply.contains("Connection: close"),
        "and asked to close, so the response ends at end-of-file:\n{reply}"
    );
    assert!(
        !reply.contains("Host: router"),
        "the caller's Host named the router, and the child is not it:\n{reply}"
    );
}

#[test]
fn a_body_is_forwarded_to_the_child_with_its_headers() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let body = "{\"model\":\"something-else\"}";
    let reply = request(serving.address(), &post("/models/gemma3/v1/echo", body));

    assert_eq!(status(&reply), Some(200), "the child answered:\n{reply}");
    assert!(
        reply.contains(&format!("body: {body}")),
        "the body itself reaches the child, byte for byte. This endpoint names \
         its model in the path and never reads the body, so what arrives here \
         is what was copied straight through:\n{reply}"
    );
    assert!(
        reply.contains(&format!("Content-Length: {}", body.len())),
        "the declared length reaches the child unchanged:\n{reply}"
    );
    assert!(
        reply.contains("Content-Type: application/json"),
        "as does every header the router has no opinion about:\n{reply}"
    );
}
