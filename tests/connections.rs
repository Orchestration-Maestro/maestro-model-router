//! How many callers are answered at once.
//!
//! Every connection is answered on a thread of its own, and nothing bounded
//! how many there were: a client that leaked connections, or a loop that
//! opened them faster than they were answered, could take threads until the
//! process could make no more. Now a stated number are answered at once. The
//! rest wait to be accepted -- queued by the operating system, which already
//! holds a backlog for exactly this -- rather than being refused, for the same
//! reason a request whose room is held waits for it: a router that queues
//! does not make every caller write a retry loop.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread::sleep;
use std::time::Duration;

mod support;
use support::{MODEL, ModelsRoot, capped, catalog_text, get, status};

#[test]
fn a_caller_past_the_limit_waits_to_be_answered_until_a_connection_ends() {
    let serving = capped(&catalog_text(""), ModelsRoot::with(&[MODEL]), 2);
    // Two callers that send nothing, each holding one of the two connections
    // the router answers at once.
    let first = TcpStream::connect(serving.address()).expect("the router accepts");
    let _second = TcpStream::connect(serving.address()).expect("the router accepts");
    sleep(Duration::from_millis(200));

    let mut third = TcpStream::connect(serving.address()).expect("the backlog accepts");
    third
        .write_all(get("/v1/models").as_bytes())
        .expect("the request is sent");
    third
        .set_read_timeout(Some(Duration::from_millis(500)))
        .expect("a read timeout");
    let mut byte = [0u8; 1];
    let early = third.read(&mut byte);
    assert!(
        early.is_err(),
        "the third caller is not answered while two connections are open: {early:?}"
    );

    drop(first);
    third
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout");
    let mut reply = String::new();
    third
        .read_to_string(&mut reply)
        .expect("answered once a connection ended");
    assert_eq!(status(&reply), Some(200), "and answered in full:\n{reply}");
}
