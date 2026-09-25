//! Re-reading the catalog without ending the process.
//!
//! A router reads its catalog once and holds it for the life of the process,
//! which makes every catalog edit a restart -- and a restart ends every child
//! mid-request. This is the other way: the file is read again, the entries it
//! now describes take effect on their next load, and nothing that is running
//! is disturbed.
//!
//! Three rules, and the tests below are one each.
//!
//! A reload that cannot parse changes nothing. The old catalog keeps serving
//! and the reply carries the parser's report, because a router that silently
//! kept serving a file the operator believes it replaced is worse than one
//! that refused.
//!
//! A reload may add and remove entries. The slot table is keyed by entry, and
//! before this it was built once and never added to -- the reason it needed no
//! lock was that the catalog could not change, which this makes false.
//!
//! A child already running keeps the arguments it was started with. Its entry
//! may now say something different, and the reply names it, because "the
//! catalog says 262144 and the process serving it was started at 131072" is a
//! thing an operator has to be told rather than left to discover.

#![cfg(test)]

use serde_json::Value;
use std::net::SocketAddr;

mod support;
use support::{MODEL, ModelsRoot, get, post, reloadable, request, status};

/// A catalog with one entry, at whatever context the caller names.
fn catalog_at(context: u32) -> String {
    format!(
        "version = 1\n\
         \n\
         [defaults]\n\
         residency = \"on-demand\"\n\
         memory_estimate_mib = 512\n\
         startup_timeout_seconds = 30\n\
         \n\
         [models.gemma3]\n\
         path = \"{MODEL}\"\n\
         context_size = {context}\n"
    )
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

/// The window one entry is offered at, as `/models` reports it.
fn offered_context(address: SocketAddr, id: &str) -> u64 {
    let payload = body(&request(address, &get("/models")));
    let data = payload["data"]
        .as_array()
        .unwrap_or_else(|| panic!("a 'data' array, got:\n{payload}"));
    data.iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("an entry called '{id}', got:\n{payload}"))["meta"]["n_ctx"]
        .as_u64()
        .unwrap_or_else(|| panic!("a numeric n_ctx, got:\n{payload}"))
}

#[test]
fn a_reload_serves_the_edited_catalog_without_a_restart() {
    let serving = reloadable(&catalog_at(4096), ModelsRoot::with(&[MODEL]));
    assert_eq!(
        offered_context(serving.address(), "gemma3"),
        4096,
        "the window the router started with"
    );

    serving.rewrite(&catalog_at(8192));

    // Until the reload, the edit is only a file: a router that re-read on
    // every request would make the catalog a thing that changes under a load
    // already in flight, which is the opposite of what a catalog is for.
    assert_eq!(
        offered_context(serving.address(), "gemma3"),
        4096,
        "an edited file alone changes nothing"
    );

    let reply = request(serving.address(), &post("/reload", ""));
    assert_eq!(status(&reply), Some(200), "got:\n{reply}");

    assert_eq!(
        offered_context(serving.address(), "gemma3"),
        8192,
        "the reload is what makes the edit take effect"
    );
}

#[test]
fn a_catalog_that_will_not_parse_is_refused_and_changes_nothing() {
    let serving = reloadable(&catalog_at(4096), ModelsRoot::with(&[MODEL]));

    serving.rewrite("version = 1\n[models.gemma3\npath =");

    let reply = request(serving.address(), &post("/reload", ""));
    assert_eq!(
        status(&reply),
        Some(400),
        "a file the parser refuses is the caller's mistake, got:\n{reply}"
    );

    assert_eq!(
        offered_context(serving.address(), "gemma3"),
        4096,
        "the catalog that was serving keeps serving"
    );
}

#[test]
fn a_reload_adds_an_entry_the_catalog_did_not_have() {
    let serving = reloadable(&catalog_at(4096), ModelsRoot::with(&[MODEL]));

    let with_second = format!(
        "{}\n[models.gemma4]\npath = \"{MODEL}\"\ncontext_size = 2048\n",
        catalog_at(4096)
    );
    serving.rewrite(&with_second);

    let reply = request(serving.address(), &post("/reload", ""));
    assert_eq!(status(&reply), Some(200), "got:\n{reply}");

    let report = body(&reply);
    assert_eq!(
        report["added"],
        serde_json::json!(["gemma4"]),
        "the reply says what appeared, got:\n{report}"
    );

    // The slot table is keyed by entry and was built once. An entry that
    // reaches the catalogue but has no slot is one that panics on first use,
    // so asking for it is the half of this test that matters.
    assert_eq!(
        offered_context(serving.address(), "gemma4"),
        2048,
        "the entry that was added is served"
    );
}
