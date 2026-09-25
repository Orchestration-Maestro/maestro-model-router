//! Raw HTTP, written and read by hand.
//!
//! Neither of this repository's two dependencies speaks HTTP, and a test
//! needs so little of it -- one request, one reply, the status line -- that a
//! third dependency would be the larger thing to trust.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// The status code from `GET /health`, or `None` when nothing answered.
///
/// Hand-written because neither of this repository's two dependencies speaks
/// HTTP. One request and one status line do not earn a third.
pub(crate) fn health(address: impl ToSocketAddrs) -> Option<u16> {
    let mut stream = TcpStream::connect(address).ok()?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    reply.split_whitespace().nth(1)?.parse().ok()
}

/// How long a test waits on a socket before calling it a hang.
///
/// Generous, because a loaded continuous-integration machine is slow and this
/// is a safety net rather than an assertion. Nothing here is timing the
/// router; the tests that do that assert their own margins.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Opens a connection that fails rather than blocks.
fn connected(address: impl ToSocketAddrs) -> TcpStream {
    let stream = TcpStream::connect(address).expect("the router is listening");
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .expect("a read timeout, so a hang fails rather than blocking the suite");
    stream
}

/// Sends one raw request and reads the whole reply as text.
///
/// Hand-written for the same reason `health` is: neither of this repository's
/// two dependencies speaks HTTP. One request and one reply do not earn a
/// third.
pub(crate) fn request(address: impl ToSocketAddrs, raw: &str) -> String {
    let mut stream = connected(address);
    stream.write_all(raw.as_bytes()).expect("write");
    let mut reply = String::new();
    // The result is dropped rather than expected: a reply the far end cut
    // short is a legitimate outcome for several of these tests, and what
    // arrived before the cut is what they assert on.
    drop(stream.read_to_string(&mut reply));
    reply
}

/// Sends one raw request and records when each chunk of the reply arrived.
///
/// The arrival times are the point. A relay that buffered a stream would
/// deliver every event in one read, and the reply text would be identical
/// either way -- which is why no test in this repository asserts a stream by
/// its content alone.
pub(crate) fn arrivals(address: impl ToSocketAddrs, raw: &str) -> (String, Vec<Duration>) {
    let mut stream = connected(address);
    stream.write_all(raw.as_bytes()).expect("write");

    let started = Instant::now();
    let mut body = String::new();
    let mut times = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                times.push(started.elapsed());
                body.push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
        }
    }
    (body, times)
}

/// A request head with no body, ready to send.
#[must_use]
pub(crate) fn get(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: router\r\nConnection: close\r\n\r\n")
}

/// A request carrying a JSON body of the length it declares.
#[must_use]
pub(crate) fn post(path: &str, body: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\n\
         Host: router\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

/// The status code a reply carries, or `None` when it carried none.
#[must_use]
pub(crate) fn status(reply: &str) -> Option<u16> {
    reply.split_whitespace().nth(1)?.parse().ok()
}
