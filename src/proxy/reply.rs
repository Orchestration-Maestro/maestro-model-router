//! What the router says in its own voice.
//!
//! Everything a caller can receive from this router that no child produced: a
//! refusal, the model listing, and a preflight answer. Split from `answer`
//! along that seam when the dispatch there grew past the module-size gate:
//! `answer` decides what a connection has earned, and this decides what that
//! looks like on the wire. What a refusal *is* -- its cause, status and code
//! -- is `refusal`'s business; this only writes one.
//!
//! The framing is the same whatever the answer: a declared length, a closed
//! connection, and the origin header that lets a page on this machine read
//! it, because a caller that can rely on none of those has to guess where the
//! reply ended.

use std::io::{self, Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

use super::endpoint::Endpoint;
use super::refusal::{Cause, Refusal};
use super::shared::Shared;

/// One complete reply the router authored. Only what varies is asked for.
struct Reply<'a> {
    status: u16,
    content_type: Option<&'a str>,
    headers: Vec<(&'static str, String)>,
    body: &'a str,
    /// Whether to send the head alone, as `HEAD` asks. The length declared
    /// is still the body's, so a client can size a buffer from it.
    head_only: bool,
}

impl Reply<'_> {
    fn write(&self, mut stream: &TcpStream) -> io::Result<()> {
        // Formatted and extended rather than written to: writing to a String
        // cannot fail, and `write!` would still hand back an error for this
        // to dismiss.
        let content_type = self
            .content_type
            .map(|content_type| format!("Content-Type: {content_type}\r\n"))
            .unwrap_or_default();
        let mut text = format!(
            "HTTP/1.1 {} {}\r\n{content_type}Content-Length: {}\r\n",
            self.status,
            reason(self.status),
            self.body.len()
        );
        text.extend(
            self.headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n")),
        );
        text.push_str("Access-Control-Allow-Origin: *\r\n");
        text.push_str("Connection: close\r\n\r\n");
        if !self.head_only {
            text.push_str(self.body);
        }
        stream.write_all(text.as_bytes())?;
        stream.flush()
    }
}

/// The reason phrase a status carries.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Gateway Timeout",
    }
}

/// Refuses a request, before a byte of any child's response was forwarded.
///
/// The headers beyond the framing are the cause's: `Retry-After` when
/// waiting changes the answer, and `Allow` when the method was the problem.
pub(super) fn refuse(stream: &TcpStream, refusal: &Refusal) -> io::Result<()> {
    let mut headers = Vec::new();
    if let Some(seconds) = refusal.cause().retry_after_seconds() {
        headers.push(("Retry-After", seconds.to_string()));
    }
    if let Cause::MethodNotAllowed(allowed) = refusal.cause() {
        headers.push(("Allow", allowed.to_owned()));
    }
    if matches!(refusal.cause(), Cause::Unauthorized) {
        headers.push(("WWW-Authenticate", "Bearer".to_owned()));
    }
    Reply {
        status: refusal.cause().status(),
        content_type: Some("application/json"),
        headers,
        body: &refusal.envelope(),
        head_only: false,
    }
    .write(stream)?;
    linger(stream);
    Ok(())
}

/// How long a refused caller's remaining bytes are read and dropped, at most.
const LINGER: Duration = Duration::from_secs(1);

/// How much of a refused request is read and dropped, at most.
const LINGER_BYTES: usize = 1024 * 1024;

/// Lets a refusal reach a caller whose request was not read to the end.
///
/// A refusal is often written before the request is: a head too large to
/// read, a body too large to take. Closing a socket that still holds unread
/// bytes resets the connection, and a Windows client that receives the reset
/// discards the reply it had not read yet, so the 431 this router just wrote
/// would arrive as nothing. The write side is closed first, which tells the
/// caller the reply is complete, and what it is still sending is read and
/// dropped for a bounded moment before the socket goes. Best effort: the
/// reply is already written, and a caller that keeps sending past the bound
/// is reset as before.
fn linger(stream: &TcpStream) {
    if stream.shutdown(Shutdown::Write).is_err() || stream.set_read_timeout(Some(LINGER)).is_err() {
        return;
    }
    let deadline = Instant::now() + LINGER;
    let mut buffer = [0u8; 8 * 1024];
    let mut drained = 0;
    while drained < LINGER_BYTES && Instant::now() < deadline {
        match (&mut &*stream).read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => drained += read,
        }
    }
}

/// Every entry the catalog carries, in the shape a client expects.
///
/// Answered from the catalog and nothing else: listing what can be served is
/// not a reason to start serving it, so no child is touched.
pub(super) fn listing(stream: &TcpStream, shared: &Shared, head_only: bool) -> io::Result<()> {
    let catalog = shared.catalog();
    let data: Vec<serde_json::Value> = catalog
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.id,
                "object": "model",
                "owned_by": "model-router",
            })
        })
        .collect();
    json(
        stream,
        &serde_json::json!({ "object": "list", "data": data }),
        head_only,
    )
}

/// A value the router authored, sent as JSON with this module's framing.
///
/// Exists so that an answer built elsewhere -- the llama.cpp-shaped ones in
/// `answer::own` -- reaches the wire the same way this module's own do, with
/// the same declared length, the same origin header and the same reading of
/// `HEAD`. The alternative was a second writer that would drift from this one
/// the first time either changed.
pub(super) fn json(
    stream: &TcpStream,
    value: &serde_json::Value,
    head_only: bool,
) -> io::Result<()> {
    text(stream, "application/json", &value.to_string(), head_only)
}

/// A body the router authored, of whatever type it says, with this module's
/// framing -- for the one answer that is not JSON, a Prometheus scrape.
pub(super) fn text(
    stream: &TcpStream,
    content_type: &str,
    body: &str,
    head_only: bool,
) -> io::Result<()> {
    Reply {
        status: 200,
        content_type: Some(content_type),
        headers: Vec::new(),
        body,
        head_only,
    }
    .write(stream)
}

/// Answers a preflight: what this endpoint accepts, from any page that can
/// reach the router.
///
/// Permissive because this header is not what decides who reaches it. On
/// loopback the origin is already the machine's own; on an address its
/// operator named, the reach is whatever the routes and the firewall allow,
/// and narrowing it here would look like a control without being one. Never a
/// child's business: a preflight asks what is allowed, and the router knows.
pub(super) fn preflight(stream: &TcpStream, endpoint: &Endpoint) -> io::Result<()> {
    let allowed = endpoint.allowed().to_owned();
    Reply {
        status: 204,
        content_type: None,
        headers: vec![
            ("Allow", allowed.clone()),
            ("Access-Control-Allow-Methods", allowed),
            ("Access-Control-Allow-Headers", "*".to_owned()),
            ("Access-Control-Max-Age", "86400".to_owned()),
        ],
        body: "",
        head_only: true,
    }
    .write(stream)
}

/// Tells a client that asked before sending its body to send it.
///
/// An interim line rather than a reply: no length, no closing, and the final
/// status still to come. A client that sent `Expect: 100-continue` waits for
/// this before it sends a byte of body, and one that is never answered either
/// hangs or gives up and sends anyway after a fixed delay -- which is what
/// every `curl` with a body over about a kilobyte was paying per request.
pub(super) fn proceed(mut stream: &TcpStream) -> io::Result<()> {
    stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn every_status_a_cause_carries_has_a_reason_phrase_of_its_own() {
        for cause in [
            Cause::MalformedRequest,
            Cause::HeadTooLarge,
            Cause::RequestTimeout,
            Cause::PathNotFound,
            Cause::MethodNotAllowed("GET"),
            Cause::ChunkedBody,
            Cause::LengthRequired,
            Cause::BodyTooLarge,
            Cause::ChildUnavailable,
            Cause::RoomContended,
        ] {
            assert_ne!(
                reason(cause.status()),
                "Gateway Timeout",
                "{cause:?} carries {} and must not fall through to the phrase \
                 kept for 504",
                cause.status()
            );
        }
        assert_eq!(reason(Cause::StartupTimeout.status()), "Gateway Timeout");
    }

    #[test]
    fn the_answers_the_router_authors_carry_their_own_reason_phrases() {
        // No cause carries these: they are the listing's status and the
        // preflight's, and a client that reads the phrase would take either
        // for a gateway that gave up.
        assert_eq!(reason(200), "OK");
        assert_eq!(reason(204), "No Content");
    }

    /// Both ends of one loopback connection: the router's, and its caller's.
    fn connection() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let caller =
            TcpStream::connect(listener.local_addr().expect("its address")).expect("a caller");
        let (router, _) = listener.accept().expect("the connection, accepted");
        (router, caller)
    }

    #[test]
    fn a_refused_caller_that_finishes_sending_is_read_to_its_end() {
        // More than one read of the drain's buffer, and less than the bound,
        // so a drain that stopped after a read or two leaves bytes behind --
        // and a socket closed on unread bytes is the reset this exists to
        // prevent.
        let (router, mut caller) = connection();
        let sending = thread::spawn(move || {
            caller
                .write_all(&vec![b'x'; 32 * 1024])
                .expect("the rest of a request");
            caller
                .shutdown(Shutdown::Write)
                .expect("the caller's side, ended");
            caller
        });

        linger(&router);
        let _caller = sending.join().expect("the caller sent everything");

        router
            .set_nonblocking(true)
            .expect("a read that cannot wait");
        assert_eq!(
            (&router).read(&mut [0u8; 16]).ok(),
            Some(0),
            "everything the caller sent was read, and the end after it"
        );
    }

    #[test]
    fn a_drain_that_reached_its_bound_stops_without_waiting_for_more() {
        // Exactly the bound, sent by a caller that then stays connected and
        // silent. A drain that did not stop at the bound would wait on it
        // until its deadline, which is a second of a thread for nothing.
        let (router, mut caller) = connection();
        let (finished, done) = mpsc::channel::<()>();
        let sending = thread::spawn(move || {
            caller
                .write_all(&vec![b'x'; LINGER_BYTES])
                .expect("the bound's worth");
            // Held open until the drain has returned, so the only way it can
            // end sooner than its deadline is the bound.
            done.recv().ok();
        });

        let started = Instant::now();
        linger(&router);
        let took = started.elapsed();
        finished.send(()).ok();
        sending.join().expect("the caller sent the bound's worth");

        assert!(
            took < LINGER / 2,
            "a drain that took all it may take stops there, rather than \
             waiting out its deadline: took {took:?}"
        );
    }

    #[test]
    fn a_caller_that_keeps_sending_is_given_up_on_at_the_deadline() {
        // A trickle: never enough to reach the bound, never a pause long
        // enough for the read to time out, and never an end. Only the
        // deadline stops a drain of this.
        let (router, mut caller) = connection();
        let (finished, done) = mpsc::channel::<()>();
        thread::spawn(move || {
            while done.recv_timeout(Duration::from_millis(50)).is_err()
                && caller.write_all(b"x").is_ok()
            {}
        });
        let (drained, ended) = mpsc::channel();
        thread::spawn(move || {
            linger(&router);
            drained.send(()).ok();
        });

        let outcome = ended.recv_timeout(LINGER * 5);
        finished.send(()).ok();
        outcome.expect("a caller that keeps sending is let go of at the deadline");
    }
}
