//! The stub server, tested on its own.
//!
//! Every supervision test reads this stub's health contract, so a stub that
//! answered wrongly would make those tests agree with each other about
//! nothing. Two properties are worth the file: the readiness transition is
//! observed rather than assumed, and a requested exit really happens with the
//! requested code.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// A port the operating system says is free. The window before the stub binds
/// it is a race, which [`serving`] answers the way `launch::server` does.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("its address")
        .port()
}

/// Kills the child when the test leaves, however it leaves.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn start(arguments: &[&str]) -> Running {
    Running(
        Command::new(env!("CARGO_BIN_EXE_stub-llama-server"))
            .args(arguments)
            .spawn()
            .expect("the stub binary is built by cargo test"),
    )
}

/// The status code from `GET /health`, or `None` while nothing is listening.
///
/// The read timeout is the one `arrivals` sets, for the same reason: a poll
/// answers `None` and comes back, where a blocked read never does. Nothing
/// listening refuses the connection, but something else holding this port may
/// accept it and then say nothing at all, and that is the case that hangs.
fn health(port: u16) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;
    stream.shutdown(Shutdown::Write).ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    reply.split_whitespace().nth(1)?.parse().ok()
}

/// How waiting for a listener ended.
///
/// A child that is gone and a child that is slow are different answers, and a
/// caller that cannot tell them apart reports the wrong one.
#[derive(Debug)]
enum Listening {
    Ready(u16),
    Exited(std::process::ExitStatus),
    Never,
}

/// Polls until the stub is listening, so the readiness assertions are not
/// racing process startup.
///
/// The child is asked after each miss, because the two reasons a port stays
/// quiet are not equally interesting. A stub that is still starting will
/// answer shortly. A stub that lost the `free_port` race is already gone, and
/// waiting out the budget for it reports a timeout where the child printed
/// the actual cause on stderr before the first poll.
fn listening(running: &mut Running, port: u16) -> Listening {
    for _ in 0..200 {
        if let Some(code) = health(port) {
            return Listening::Ready(code);
        }
        match running.0.try_wait() {
            Ok(Some(status)) => return Listening::Exited(status),
            Ok(None) => {}
            Err(error) => panic!("cannot ask whether the stub is still running: {error}"),
        }
        sleep(Duration::from_millis(25));
    }
    Listening::Never
}

/// Starts the stub with `arguments` on a free port and waits until it
/// listens. Returns it, the port, and the first health status it answered.
///
/// Something can take the port between `free_port` releasing it and the stub
/// binding it, and the stub then exits on the bind, saying so on stderr.
/// `launch::server` answers that race by starting a child once more on a
/// fresh port, and this answers it the same way: once, so a stub that loses
/// twice still fails the test, with the reason.
fn serving(arguments: &[&str]) -> (Running, u16, u16) {
    let mut lost_once = false;
    loop {
        let (mut running, port) = on_a_free_port(arguments);
        match listening(&mut running, port) {
            Listening::Ready(code) => return (running, port, code),
            // A failed bind is the only exit with code 1 these tests can
            // cause: none of them asks the stub for that code.
            Listening::Exited(status) if status.code() == Some(1) && !lost_once => {
                lost_once = true;
            }
            Listening::Exited(status) => panic!(
                "the stub exited with {status} rather than listening on port {port}; \
                 it printed the reason on stderr, and losing the free_port race \
                 reads `cannot bind ...: Address already in use`"
            ),
            Listening::Never => panic!("the stub never listened on port {port}"),
        }
    }
}

#[test]
fn health_reports_loading_until_the_readiness_moment_then_ready() {
    let (_running, port, first) = serving(&["--host", "127.0.0.1", "--ready-after", "1500"]);

    assert_eq!(
        first, 503,
        "a loading server answers 503, which is what llama-server does"
    );

    for _ in 0..200 {
        if health(port) == Some(200) {
            return;
        }
        sleep(Duration::from_millis(25));
    }
    panic!("the stub never became ready");
}

#[test]
fn unknown_arguments_are_ignored_so_a_real_invocation_drives_the_stub() {
    let (_running, _, first) = serving(&[
        "--model",
        "somewhere/a.gguf",
        "--jinja",
        "--ctx-size",
        "4096",
        "--host",
        "127.0.0.1",
    ]);

    assert_eq!(
        first, 200,
        "ready immediately, and every flag it does not know stepped over"
    );
}

#[test]
fn a_path_the_stub_does_not_serve_is_not_found() {
    let (_running, port, _) = serving(&[]);

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .write_all(b"GET /v1/models HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write");
    stream.shutdown(Shutdown::Write).expect("shutdown");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).expect("read");

    assert!(
        reply.starts_with("HTTP/1.1 404 "),
        "only /health is served:\n{reply}"
    );
}

/// Starts the stub with `arguments` on a port the operating system says is
/// free, and says which.
fn on_a_free_port(arguments: &[&str]) -> (Running, u16) {
    let port = free_port();
    let port_text = port.to_string();
    let mut invocation = vec!["--port", port_text.as_str()];
    invocation.extend_from_slice(arguments);
    (start(&invocation), port)
}

/// How the stub started with `arguments` on a free port exited.
fn exit_status(arguments: &[&str]) -> std::process::ExitStatus {
    let (mut running, _) = on_a_free_port(arguments);
    running.0.wait().expect("the stub exits on its own")
}

#[test]
fn exit_after_exits_with_the_requested_code() {
    // The exit alone is waited for. A stub that lives 250 ms can be gone
    // before a readiness poll lands -- Windows takes its time refusing a
    // connection to a port nobody listens on yet -- and a poll would report
    // as never listening a stub that did.
    let arguments = [
        "--ready-after",
        "60000",
        "--exit-after",
        "250",
        "--exit-code",
        "9",
    ];
    let mut status = exit_status(&arguments);
    if status.code() == Some(1) {
        // free_port's race, lost once: started again, as `serving` does.
        status = exit_status(&arguments);
    }
    assert_eq!(
        status.code(),
        Some(9),
        "the exit code a test asked for, so a crash during loading can be driven"
    );
}

/// The stub exits the moment it cannot bind, saying so on stderr. Waiting for
/// it to listen anyway spends the whole readiness budget and then reports a
/// timeout, which names the wrong cause -- and where whatever took the port
/// accepts without answering, the wait never returns at all.
///
/// This is the `free_port` race [`serving`] retries once. Retried it may be;
/// unreadable it should not be.
#[test]
fn a_stub_that_cannot_bind_is_reported_as_gone_rather_than_as_slow() {
    // Held for the whole test, which is what losing the race to another
    // listener looks like from here.
    let holder = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = holder.local_addr().expect("its address").port();

    let mut running = start(&["--port", &port.to_string()]);
    let started = Instant::now();
    let outcome = listening(&mut running, port);
    let took = started.elapsed();

    assert!(
        matches!(outcome, Listening::Exited(_)),
        "a stub that is already gone is reported as gone, not handed to the \
         readiness budget; got {outcome:?} after {took:?}"
    );
    assert!(
        took < Duration::from_secs(2),
        "and it is reported as soon as the child is gone, rather than after \
         the full budget; took {took:?}"
    );
}

/// Sends one request and records when each chunk of the reply arrived.
///
/// The arrival times are the point: a stub that buffered its own output would
/// deliver every event at once, which would make the router's streaming test
/// agree with a broken relay.
fn arrivals(port: u16, request_line: &str) -> (String, Vec<Duration>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout, so a hang fails rather than blocks");
    write!(
        stream,
        "{request_line} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    // Not half-closed: to a silent stub, as to `llama-server`, a client that
    // closes its end is one that left.

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

#[test]
fn a_paced_stream_arrives_spread_out_rather_than_at_once() {
    let (_running, port, _) = serving(&["--stream-events", "5", "--stream-gap", "100"]);

    let (body, times) = arrivals(port, "POST /v1/chat/completions");

    assert!(
        body.contains("data: {\"n\":0}") && body.contains("data: {\"n\":4}"),
        "every event arrives:\n{body}"
    );
    let first = *times.first().expect("at least one arrival");
    let last = *times.last().expect("at least one arrival");
    assert!(
        last.saturating_sub(first) >= Duration::from_millis(250),
        "five events paced 100ms apart spread over at least half their \
         production time; arrivals were {times:?}"
    );
}

#[test]
fn a_first_byte_delay_keeps_the_stream_silent_until_it_passes() {
    let (_running, port, _) = serving(&["--stream-events", "1", "--first-byte-after", "400"]);

    let (body, times) = arrivals(port, "POST /v1/chat/completions");

    assert!(
        body.contains("data: {\"n\":0}"),
        "the event arrives:\n{body}"
    );
    let first = *times.first().expect("at least one arrival");
    assert!(
        first >= Duration::from_millis(350),
        "nothing arrives before the delay has passed, the way a model \
         reading a long prompt says nothing; the first byte came at {first:?}"
    );
}

#[test]
fn a_client_that_leaves_during_the_silence_is_noticed_before_it_ends() {
    // llama-server looks for a closed connection while a request is in
    // progress, and cancels the request when it finds one. Where the router
    // cannot wake its own blocked read -- Windows cannot -- that is how a
    // model whose caller hung up is released at all, so a stub that noticed
    // only on its first write would hold the model for the whole silence.
    let marker = std::env::temp_dir().join(format!(
        "model-router-silent-hangup-marker-{}",
        std::process::id()
    ));
    drop(std::fs::remove_file(&marker));
    let marker_text = marker.display().to_string();
    let (_running, port, _) = serving(&[
        "--stream-events",
        "1",
        "--first-byte-after",
        "10000",
        "--hangup-marker",
        &marker_text,
    ]);

    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    client
        .write_all(
            b"POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        )
        .expect("the request, written");
    drop(client);

    let left = Instant::now();
    while !marker.exists() && left.elapsed() < Duration::from_secs(3) {
        sleep(Duration::from_millis(25));
    }
    let noticed = marker.exists();
    drop(std::fs::remove_file(&marker));
    assert!(
        noticed,
        "the stub noticed its client leave within three seconds of a \
         ten-second silence"
    );
}

#[test]
fn die_after_events_truncates_the_stream() {
    let (_running, port, _) = serving(&[
        "--stream-events",
        "10",
        "--stream-gap",
        "20",
        "--die-after-events",
        "2",
    ]);

    let (body, _) = arrivals(port, "POST /v1/chat/completions");

    assert!(
        body.contains("data: {\"n\":1}"),
        "the events before the death arrive:\n{body}"
    );
    assert!(
        !body.contains("data: {\"n\":2}"),
        "and nothing after it does:\n{body}"
    );
}

#[test]
fn echo_reflects_the_request_line_and_every_header() {
    let (_running, port, _) = serving(&[]);

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout");
    stream
        .write_all(
            b"GET /v1/echo HTTP/1.1\r\n\
              Host: localhost\r\n\
              X-Written-By: the test\r\n\
              Connection: close\r\n\r\n",
        )
        .expect("write");
    stream.shutdown(Shutdown::Write).expect("shutdown");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).expect("read");

    assert!(reply.starts_with("HTTP/1.1 200 "), "echo answers:\n{reply}");
    assert!(
        reply.contains("GET /v1/echo HTTP/1.1"),
        "the request line comes back:\n{reply}"
    );
    assert!(
        reply.contains("X-Written-By: the test"),
        "and every header with it:\n{reply}"
    );
}

#[test]
fn echo_reports_the_alias_the_stub_was_started_as() {
    let (_running, port, _) = serving(&["--alias", "gemma3"]);

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a read timeout");
    stream
        .write_all(b"GET /v1/echo HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write");
    stream.shutdown(Shutdown::Write).expect("shutdown");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).expect("read");

    // Observed here from the stub directly, so that when a router test asserts
    // which child answered, a failure means the router rather than the stub.
    assert!(
        reply.contains("alias: gemma3"),
        "the echo says which entry this child is:\n{reply}"
    );
}
