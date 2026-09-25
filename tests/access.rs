//! Who may use the router.
//!
//! Open by default: the router answers whoever reaches an address it listens
//! on. Two settings narrow that. `MAESTRO_API_KEY` is a key every request
//! carries as `Authorization: Bearer <key>`. `MAESTRO_ALLOWED_ORIGINS` lists
//! the origins a browser page may call it from, because a router that
//! approves every origin's preflight is one any page open in a browser can
//! drive. Both are read through `Access::from_variables`, so no test changes
//! the process environment.

#![cfg(test)]

use maestro_model_router::proxy::Access;

mod support;
use support::{MODEL, ModelsRoot, catalog_text, guarded, request, status};

/// A request for the listing, with extra header lines.
fn listing(headers: &str) -> String {
    format!("GET /v1/models HTTP/1.1\r\nHost: router\r\n{headers}Connection: close\r\n\r\n")
}

fn keyed(key: &str) -> Access {
    Access::from_variables(Some(key.into()), None)
}

fn from(origins: &str) -> Access {
    Access::from_variables(None, Some(origins.into()))
}

#[test]
fn once_a_key_is_set_a_request_without_it_is_refused_as_unauthorized() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        keyed("s3cret"),
    );

    for headers in [
        "",
        "Authorization: Bearer wrong\r\n",
        "Authorization: Bearer s3cre\r\n",
    ] {
        let reply = request(serving.address(), &listing(headers));
        assert_eq!(status(&reply), Some(401), "{headers:?}:\n{reply}");
        assert!(reply.contains("\"code\":\"invalid_api_key\""), "{reply}");
        assert!(
            reply.contains("WWW-Authenticate: Bearer"),
            "says how to authenticate:\n{reply}"
        );
    }
}

#[test]
fn a_request_carrying_the_key_is_served() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        keyed("s3cret"),
    );
    let reply = request(
        serving.address(),
        &listing("Authorization: Bearer s3cret\r\n"),
    );
    assert_eq!(status(&reply), Some(200), "{reply}");
}

#[test]
fn a_preflight_needs_no_key_because_a_browser_sends_none() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        keyed("s3cret"),
    );
    let reply = request(
        serving.address(),
        "OPTIONS /v1/models HTTP/1.1\r\nHost: router\r\nOrigin: http://page.example\r\n\r\n",
    );
    assert_eq!(status(&reply), Some(204), "{reply}");
}

#[test]
fn an_origin_not_on_the_list_is_refused_and_one_on_it_is_served() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        from("http://allowed.example, http://also.example"),
    );

    let refused = request(
        serving.address(),
        &listing("Origin: http://evil.example\r\n"),
    );
    assert_eq!(status(&refused), Some(403), "{refused}");
    assert!(
        refused.contains("\"code\":\"origin_not_allowed\""),
        "{refused}"
    );

    let served = request(
        serving.address(),
        &listing("Origin: http://also.example\r\n"),
    );
    assert_eq!(status(&served), Some(200), "{served}");
}

#[test]
fn a_caller_that_is_no_browser_sends_no_origin_and_is_served() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        from("http://allowed.example"),
    );
    let reply = request(serving.address(), &listing(""));
    assert_eq!(status(&reply), Some(200), "{reply}");
}

#[test]
fn a_preflight_from_an_origin_not_on_the_list_is_refused() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        from("http://allowed.example"),
    );
    let reply = request(
        serving.address(),
        "OPTIONS /v1/models HTTP/1.1\r\nHost: router\r\nOrigin: http://evil.example\r\n\r\n",
    );
    assert_eq!(status(&reply), Some(403), "{reply}");
}

#[test]
fn empty_settings_leave_the_router_open() {
    let serving = guarded(
        &catalog_text(""),
        ModelsRoot::with(&[MODEL]),
        Access::from_variables(Some("".into()), Some(" , ".into())),
    );
    let reply = request(
        serving.address(),
        &listing("Origin: http://any.example\r\n"),
    );
    assert_eq!(
        status(&reply),
        Some(200),
        "an empty key and a list naming nothing are unset, not a lock with no \
         way in:\n{reply}"
    );
}
