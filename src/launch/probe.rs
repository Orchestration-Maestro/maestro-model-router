//! Asking a child whether it is ready to answer.
//!
//! One request, one status line, and nothing else read from the reply. This
//! is deliberately not the beginning of an HTTP client: the slice that proxies
//! needs streamed responses, connection reuse and concurrency, and should
//! choose a library against those requirements rather than inherit one picked
//! for a status code. Keeping the probe behind the launch module's interface
//! is what makes replacing it later a local change.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// How long one probe may take before it is treated as no answer.
///
/// Both halves are bounded. A connect that hangs and a reply that never
/// arrives are the same thing from here, and neither may outlive the poll it
/// belongs to.
const TIMEOUT: Duration = Duration::from_secs(2);

/// The status code from `GET /health`, or `None` when nothing answered.
///
/// `llama-server` answers 503 while a model is loading and 200 once it will
/// serve, which is the whole contract this slice depends on.
pub(super) fn health(address: SocketAddr) -> Option<u16> {
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;

    let mut status = String::new();
    BufReader::new(stream).read_line(&mut status).ok()?;
    // `HTTP/1.1 200 OK`: the second word, and none of the rest.
    status.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn the_status_is_the_second_word_of_the_reply_line() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the port's address");
        let answering = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the probe connects");
            let mut request = [0; 512];
            drop(stream.read(&mut request));
            stream
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\n\r\n")
                .expect("the reply, written");
        });

        assert_eq!(health(address), Some(503), "a model still loading");
        answering.join().expect("the reply was sent");
    }
}
