//! What the router is holding, in the text format Prometheus scrapes.
//!
//! `/models` answers the same questions for a llama.cpp client, one request
//! at a time. A scrape asks them every few seconds and keeps the answers, so
//! a graph can show what was loaded when the machine ran short -- which is
//! the question the journal answered only for whoever read it at the time.

use crate::support::{
    MODEL, ModelsRoot, budgeted, catalog_text, get, post, request, serving, status,
};

/// Two entries, so a scrape can say one is loaded and the other is not.
fn two_entries() -> String {
    catalog_text(&format!("\n[models.other]\npath = \"{MODEL}\"\n"))
}

/// The body of a scrape, having checked it is one.
fn scrape(address: std::net::SocketAddr) -> String {
    let reply = request(address, &get("/metrics"));
    assert_eq!(status(&reply), Some(200), "got:\n{reply}");
    assert!(
        reply.contains("Content-Type: text/plain; version=0.0.4"),
        "the exposition format's own type, got:\n{reply}"
    );
    reply
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default()
}

#[test]
fn a_scrape_says_which_entries_are_loaded() {
    let serving = serving(&two_entries(), ModelsRoot::with(&[MODEL]));
    let loaded = request(
        serving.address(),
        &post("/models/load", r#"{"model":"gemma3"}"#),
    );
    assert_eq!(status(&loaded), Some(200), "got:\n{loaded}");

    let body = scrape(serving.address());

    assert!(
        body.contains("model_router_model_loaded{model=\"gemma3\"} 1\n"),
        "got:\n{body}"
    );
    assert!(
        body.contains("model_router_model_loaded{model=\"other\"} 0\n"),
        "got:\n{body}"
    );
}

#[test]
fn a_scrape_starts_nothing() {
    let serving = serving(&two_entries(), ModelsRoot::with(&[MODEL]));

    scrape(serving.address());

    assert!(
        serving.loaded().is_empty(),
        "loaded: {:?}",
        serving.loaded()
    );
}

#[test]
fn a_scrape_carries_what_each_entry_is_estimated_to_hold() {
    let serving = serving(&two_entries(), ModelsRoot::with(&[MODEL]));

    let body = scrape(serving.address());

    assert!(
        body.contains("model_router_model_declared_mib{model=\"gemma3\"} 512\n"),
        "got:\n{body}"
    );
    assert!(
        body.contains("# TYPE model_router_model_declared_mib gauge\n"),
        "got:\n{body}"
    );
}

#[test]
fn a_scrape_carries_the_budget_and_the_line_waiting_for_room() {
    let serving = budgeted(&two_entries(), ModelsRoot::with(&[MODEL]), Some(600));

    let body = scrape(serving.address());

    assert!(
        body.contains("model_router_memory_budget_mib 600\n"),
        "got:\n{body}"
    );
    assert!(
        body.contains("model_router_requests_waiting 0\n"),
        "got:\n{body}"
    );
}

#[test]
fn a_router_with_no_budget_reports_none_rather_than_zero() {
    let serving = budgeted(&two_entries(), ModelsRoot::with(&[MODEL]), None);

    let body = scrape(serving.address());

    assert!(
        !body.contains("model_router_memory_budget_mib"),
        "got:\n{body}"
    );
}

#[test]
fn a_scrape_names_the_build_answering_it() {
    let serving = serving(&two_entries(), ModelsRoot::with(&[MODEL]));

    let body = scrape(serving.address());

    let version = env!("CARGO_PKG_VERSION");
    assert!(
        body.contains(&format!(
            "model_router_build_info{{version=\"{version}\",commit=\""
        )),
        "got:\n{body}"
    );
}
