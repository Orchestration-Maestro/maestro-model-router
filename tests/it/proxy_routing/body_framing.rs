//! How a request body arrives: the length it declares, the leave it asks
//! for with `Expect: 100-continue`, and the encodings the router refuses.
//!
//! These are refused or met before any child sees the request, where a
//! status is still possible.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::support::{MODEL, ModelsRoot, catalog_text, request, serving, status};

#[test]
fn an_expectation_with_no_body_to_send_is_answered_without_an_interim_line() {
    // `Expect: 100-continue` asks leave to send a body. With none declared
    // there is nothing to wait for, and a `100` would be a line the client
    // then has to read past before the answer it asked for.
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        "POST /models/gemma3/v1/echo HTTP/1.1\r\n\
         Host: router\r\n\
         Content-Length: 0\r\n\
         Expect: 100-continue\r\n\
         Connection: close\r\n\
         \r\n",
    );

    assert!(
        reply.starts_with("HTTP/1.1 200 "),
        "the answer came first, with no interim line before it:\n{reply}"
    );
}

#[test]
fn a_length_the_router_cannot_read_is_refused_rather_than_treated_as_none() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    // Defaulting this to zero would forward the header as received and leave
    // the child waiting for a body nobody is going to send.
    let reply = request(
        serving.address(),
        "POST /models/gemma3/v1/echo HTTP/1.1\r\n\
         Host: router\r\n\
         Content-Length: abc\r\n\
         Connection: close\r\n\
         \r\n",
    );

    assert_eq!(
        status(&reply),
        Some(400),
        "a length that will not parse is refused where a status is still \
         possible:\n{reply}"
    );
    assert!(
        reply.contains("abc"),
        "quoting back what arrived, so the reader sees what the router saw:\n{reply}"
    );
}

#[test]
fn a_body_larger_than_the_router_will_read_is_refused_before_it_is_read() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    // Declared and not sent. A router that read before deciding would block
    // here waiting for sixty-four megabytes that are never coming; one that
    // allocated before deciding would take them from a stranger's say-so.
    let declared = 64 * 1024 * 1024;
    let reply = request(
        serving.address(),
        &format!(
            "POST /v1/chat/completions HTTP/1.1\r\n\
             Host: router\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {declared}\r\n\
             Connection: close\r\n\
             \r\n"
        ),
    );

    assert_eq!(
        status(&reply),
        Some(413),
        "an oversized body has its own status, not the 400 that every other \
         unreadable body gets:\n{reply}"
    );
    assert!(
        reply.contains(&declared.to_string()) && reply.contains("this router will read"),
        "the refusal names what was declared and what the limit is, so the \
         reader knows which of the two to change:\n{reply}"
    );
}

/// Reads until the first blank line, which is where an interim response ends.
fn read_head(stream: &mut TcpStream) -> String {
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while !seen.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => seen.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&seen).into_owned()
}

#[test]
fn a_client_that_asks_before_sending_its_body_is_told_to_send_it() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    // Both endpoints, because they read the body at different moments: the
    // generic one before it knows which model answers, the dedicated one
    // only once a child is ready to be handed it.
    for path in ["/v1/echo", "/models/gemma3/v1/echo"] {
        let body = "{\"model\":\"gemma3\"}";
        let mut stream = TcpStream::connect(serving.address()).expect("the router is listening");
        // The deadline is the assertion: a client honouring `Expect` sends
        // nothing until it is told to, so a router that never tells it is a
        // request that never completes.
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a read timeout");
        write!(
            stream,
            "POST {path} HTTP/1.1\r\n\
             Host: router\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Expect: 100-continue\r\n\
             Connection: close\r\n\
             \r\n",
            body.len()
        )
        .expect("write");

        let interim = read_head(&mut stream);
        assert!(
            interim.starts_with("HTTP/1.1 100 Continue\r\n"),
            "the router tells the client to send the body it is holding back \
             ({path}); curl waits a second for this and then sends anyway, and \
             a stricter client waits forever:\n{interim:?}"
        );

        stream.write_all(body.as_bytes()).expect("write the body");
        let mut reply = String::new();
        drop(stream.read_to_string(&mut reply));

        assert_eq!(
            status(&reply),
            Some(200),
            "the child answered ({path}):\n{reply}"
        );
        assert!(
            reply.contains(&format!("body: {body}")),
            "and received the body that was sent on request ({path}):\n{reply}"
        );
        assert!(
            !reply.to_lowercase().contains("expect:"),
            "the expectation was the router's to meet, so the child is not \
             asked to meet it again ({path}):\n{reply}"
        );
    }
}

#[test]
fn a_chunked_request_body_is_refused_rather_than_mangled() {
    let serving = serving(&catalog_text(""), ModelsRoot::with(&[MODEL]));

    let reply = request(
        serving.address(),
        "POST /models/gemma3/v1/chat/completions HTTP/1.1\r\n\
         Host: router\r\n\
         Transfer-Encoding: chunked\r\n\
         Connection: close\r\n\
         \r\n\
         0\r\n\r\n",
    );

    assert_eq!(
        status(&reply),
        Some(501),
        "decoding a chunked body to re-encode it upstream is work with no \
         caller:\n{reply}"
    );
    assert!(
        reply.contains("gemma3"),
        "and the refusal still names the entry it was about:\n{reply}"
    );
}
