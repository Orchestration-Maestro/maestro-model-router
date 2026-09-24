//! Copying bytes between the caller's connection and the child's.
//!
//! The heart of the slice, and the shortest module in it. That is the point:
//! everything this file does not do -- parse a response, frame it, buffer it,
//! decide when it is complete -- is a way a stream could stop being one.
//!
//! **Upstream** is the connection to the child, **downstream** the connection
//! to the caller. Every failure below says which side it happened on, because
//! the two mean opposite things: an upstream failure is a model that stopped
//! answering, a downstream one is a caller that walked away.
//!
//! Nothing here is buffered in order to keep a late error available. Once a
//! status line has been forwarded, a proxy cannot retract it and send `502`
//! instead, and a router that held a response back so it could still change
//! its mind would trade the property this slice exists for against a nicer
//! message.

use std::io::{BufReader, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::head::Head;
use crate::launch::Child;

/// How much is moved per read.
///
/// Small on purpose. Every read is written and flushed downstream before the
/// next one is attempted, so this is the largest a single event can be delayed
/// by -- not a throughput setting.
const BUFFER: usize = 8 * 1024;

/// Forwards one request and copies the response back as it arrives.
///
/// # Errors
///
/// Returns whatever the sockets returned while the request was being sent.
/// Once the response has begun, failures on either side end the relay rather
/// than propagating: there is no status left to send, and the caller's client
/// library is built to notice a closed connection.
pub(super) fn run(
    head: &Head,
    child: &Child,
    body: Option<&[u8]>,
    reader: &mut BufReader<TcpStream>,
    downstream: &mut TcpStream,
) -> std::io::Result<()> {
    let endpoint = child.endpoint();
    let mut upstream = TcpStream::connect(endpoint)?;
    upstream.write_all(head.rewrite(endpoint).as_bytes())?;
    match body {
        // Already read, because the generic endpoint had to look inside it to
        // learn which model answers. Written from what was read rather than
        // from the socket, which has nothing left on it.
        Some(bytes) => upstream.write_all(bytes)?,
        // Never read, so it is copied straight through. The dedicated
        // endpoint names its model in the path and has no reason to look.
        None => forward_body(head, reader, &mut upstream)?,
    }
    upstream.flush()?;

    copy_response(&mut upstream, downstream);
    // Dropped here whatever happened, which closes it. A closed connection is
    // how `llama-server` is told to stop generating, so a caller that hung up
    // does not leave a model producing an answer nobody reads.
    Ok(())
}

/// Copies exactly the body the caller declared.
///
/// Chunked, in the loop sense: a declared length is trusted for how much to
/// read, never for how much to allocate. A `Content-Length` of four gigabytes
/// is a header, not a reason to reserve four gigabytes.
fn forward_body(
    head: &Head,
    reader: &mut BufReader<TcpStream>,
    upstream: &mut TcpStream,
) -> std::io::Result<()> {
    let mut remaining = head.body_bytes();
    let mut buffer = [0u8; BUFFER];
    while remaining > 0 {
        let want = remaining.min(BUFFER);
        let read = reader.read(&mut buffer[..want])?;
        if read == 0 {
            // The caller declared more than it sent. The child is given what
            // arrived and decides for itself; guessing here would be this
            // router having an opinion about a body it does not read.
            break;
        }
        upstream.write_all(&buffer[..read])?;
        remaining -= read;
    }
    Ok(())
}

/// How often the watch on a caller looks up to see whether the relay ended.
///
/// Also how long a finished relay can wait for its watch to notice, which is
/// why it is short: the reply has been written by then, so nothing waits on
/// it but a thread.
const WATCH: Duration = Duration::from_millis(100);

/// Copies the response until the child closes, flushing after every read.
///
/// The flush is the whole slice. A buffered writer that flushed when its
/// buffer filled would batch a stream into one delivery, and the reply text
/// would be identical either way -- which is why `tests/streaming.rs` asserts
/// when bytes arrive rather than what they say.
///
/// The caller is watched while this runs. A write that fails tells the relay
/// its caller left, but only once there is something to write, and a model
/// reading a long prompt or finishing a reply it does not stream writes
/// nothing for minutes. Without the watch, such a model stayed busy and kept
/// generating for nobody until it spoke.
fn copy_response(upstream: &mut TcpStream, downstream: &mut TcpStream) {
    let relaying = Arc::new(AtomicBool::new(true));
    let watch = watch(downstream, upstream, &relaying);
    relay_response(upstream, downstream);
    relaying.store(false, Ordering::Relaxed);
    // Wakes the watch where the platform lets a shutdown do that. Only the
    // reading half: the caller must not see its reply end while the model is
    // still held, or a caller quick to ask again finds it busy. The owner of
    // the connection ends it, once it has let the model go.
    drop(downstream.shutdown(Shutdown::Read));
    if let Some(watch) = watch {
        drop(watch.join());
    }
}

/// Watches the caller's side of the connection while a response is relayed,
/// and closes the child's side when the caller leaves.
///
/// A caller that closes its end is gone: `llama-server` reads it the same way,
/// and closing the child's connection is how it is told to stop generating.
/// Bytes a caller sends after its request are not this router's to read --
/// every connection answers one request -- and are dropped.
///
/// Closing the child's side also wakes the relay's read on Linux and macOS.
/// Windows does not wake a read blocked on a socket that is shut down
/// (rust-lang/rust#121594), so there the relay ends when the child closes the
/// connection in turn, which `llama-server` does once it next looks -- about
/// once a second.
///
/// `None` when the sockets cannot be duplicated, in which case the relay runs
/// as it did before the watch existed: a caller that leaves is noticed on the
/// next write.
fn watch(
    downstream: &TcpStream,
    upstream: &TcpStream,
    relaying: &Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    let caller = downstream.try_clone().ok()?;
    let child = upstream.try_clone().ok()?;
    caller.set_read_timeout(Some(WATCH)).ok()?;
    let relaying = Arc::clone(relaying);
    Some(thread::spawn(move || {
        let mut byte = [0u8; 1];
        while relaying.load(Ordering::Relaxed) {
            match (&caller).read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) => {}
                Err(_) => break,
            }
        }
        if relaying.load(Ordering::Relaxed) {
            drop(child.shutdown(Shutdown::Both));
        }
    }))
}

/// The copy itself: every read written and flushed before the next.
fn relay_response(upstream: &mut TcpStream, downstream: &mut TcpStream) {
    let mut buffer = [0u8; BUFFER];
    loop {
        // End-of-file and a broken upstream are one arm because they are one
        // action. A response that ended and one that stopped differ in what
        // they mean, not in what is left to do: no status can be sent once
        // bytes have been forwarded, so both close the connection and let the
        // caller's client library see a complete or truncated answer for
        // itself.
        let read = match upstream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };

        if downstream.write_all(&buffer[..read]).is_err() || downstream.flush().is_err() {
            // The caller hung up mid-answer. Returning drops the upstream
            // socket, which stops the child generating.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// Both ends of one loopback connection.
    fn connection() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("its address");
        let near = TcpStream::connect(address).expect("a connection");
        let (far, _) = listener.accept().expect("the connection, accepted");
        (near, far)
    }

    #[test]
    fn the_caller_sees_its_reply_end_only_once_its_connection_is_let_go() {
        // The connection's owner releases the model before it lets the
        // connection go, so a caller that reads to the end finds the model
        // idle. A relay that ended the connection itself did so while the
        // model was still held, and a caller quick to ask again found it busy.
        let (mut upstream, mut child) = connection();
        child
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .expect("the reply, written");
        drop(child);
        let (mut downstream, mut caller) = connection();

        copy_response(&mut upstream, &mut downstream);

        caller
            .set_read_timeout(Some(Duration::from_millis(300)))
            .expect("a read timeout");
        let mut reply = Vec::new();
        let mut buffer = [0u8; 256];
        let ended = loop {
            match caller.read(&mut buffer) {
                Ok(0) => break true,
                Ok(read) => reply.extend_from_slice(&buffer[..read]),
                Err(_) => break false,
            }
        };
        assert!(reply.ends_with(b"ok"), "the whole reply arrived: {reply:?}");
        assert!(
            !ended,
            "the caller saw its connection end while its owner still held it"
        );

        drop(downstream);
        assert_eq!(
            caller.read(&mut buffer).ok(),
            Some(0),
            "and saw it end once it was let go"
        );
    }
}
