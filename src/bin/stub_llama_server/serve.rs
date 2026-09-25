//! Running the stub as it was asked to: exiting before the bind, serving,
//! and exiting on a timer.

use std::fs;
use std::net::TcpListener;
use std::process::ExitCode;
use std::thread;
use std::time::Instant;

use crate::options::Options;
use crate::reply::{self, Pacing};

/// Runs the stub as `options` ask, and ends with the code they ask for.
pub(crate) fn run(options: Options) -> ExitCode {
    // Checked before the bind that would otherwise make this run
    // indistinguishable from a real one: a marker not yet there means this is
    // the first run, and it exits having touched neither the port nor the
    // socket, so nothing outside this process can tell it apart from having
    // lost `free_port`'s race. Present means a prior run already paid that
    // cost, so this one behaves as asked.
    if let Some(marker) = &options.never_bind_marker {
        if !marker.exists() {
            if let Err(error) = fs::write(marker, b"") {
                eprintln!("stub-llama-server: cannot write never-bind marker: {error}");
            }
            return ExitCode::FAILURE;
        }
    } else if options.never_bind {
        return ExitCode::FAILURE;
    }

    let listener = match TcpListener::bind((options.host.as_str(), options.port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "stub-llama-server: cannot bind {}:{}: {error}",
                options.host, options.port
            );
            return ExitCode::FAILURE;
        }
    };

    // Exiting is a timer rather than a branch in the accept loop: a crash
    // during loading has to be observable while the socket is still being
    // served, which is exactly what a test drives with this. The timer is
    // this thread and the socket is served on another, because returning
    // from `main` is how the process ends with the code it was asked for.
    let Some(after) = options.exit_after else {
        return serve(&listener, &options);
    };
    let code = options.exit_code;
    thread::spawn(move || serve(&listener, &options));
    thread::sleep(after);
    // Said on the way out, as a server that fails to load says why: what the
    // router keeps of a child's output is what a test reads.
    eprintln!(
        "stub-llama-server: exiting with code {code} after {} ms, as asked",
        after.as_millis()
    );
    ExitCode::from(code)
}

/// Accepts until the process ends, one thread per connection.
///
/// Threaded because a paced stream holds its connection for as long as it
/// runs. Answering in the accept loop would make a second caller wait out the
/// first one's stream, which is a property no real server has and no test
/// should have to work around.
fn serve(listener: &TcpListener, options: &Options) -> ExitCode {
    let started = Instant::now();
    for stream in listener.incoming().flatten() {
        let ready = started.elapsed() >= options.ready_after;
        let pacing = Pacing {
            first_byte_after: options.pacing.first_byte_after,
            events: options.pacing.events,
            gap: options.pacing.gap,
            die_after: options.pacing.die_after,
            hangup_marker: options.pacing.hangup_marker.clone(),
        };
        let alias = options.alias.clone();
        thread::spawn(move || {
            // A failed reply is a client that hung up. Nothing here is
            // durable, so the next connection is the only thing that matters.
            drop(reply::answer(stream, ready, &pacing, &alias));
        });
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::env::temp_dir;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::options;

    fn parsed(arguments: &[&str]) -> Options {
        options::parse(arguments.iter().map(|argument| (*argument).to_owned())).expect("parses")
    }

    /// Runs the stub against a marker named for `test`, as asked to exit with
    /// code 7 the moment it is serving, so a run that got past the marker ends
    /// rather than serving forever.
    fn run_with_marker(test: &str, present: bool) -> (ExitCode, bool) {
        let marker = temp_dir().join(format!("stub-never-bind-{test}-{}", process::id()));
        drop(fs::remove_file(&marker));
        if present {
            fs::write(&marker, b"").expect("the marker, written");
        }
        let options = parsed(&[
            "--never-bind-marker",
            &marker.display().to_string(),
            "--exit-after",
            "0",
            "--exit-code",
            "7",
        ]);
        // On a thread with a deadline, so a run that serves on rather than
        // exiting fails this test instead of holding the whole test binary.
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || sender.send(run(options)).ok());
        let code = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the run ends within five seconds");
        let left = marker.exists();
        drop(fs::remove_file(&marker));
        (code, left)
    }

    #[test]
    fn a_first_run_without_its_marker_exits_before_binding_and_leaves_the_marker() {
        let (code, left) = run_with_marker("absent", false);

        assert_eq!(code, ExitCode::FAILURE, "the first run never serves");
        assert!(left, "and leaves the marker for the run after it");
    }

    #[test]
    fn a_run_that_finds_its_marker_serves_as_asked() {
        let (code, _) = run_with_marker("present", true);

        assert_eq!(
            code,
            ExitCode::from(7),
            "it got as far as the asked-for exit"
        );
    }

    /// The status `GET /health` answers from a stub serving with `arguments`.
    fn health_from(arguments: &[&str]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("its address").port();
        let options = parsed(arguments);
        thread::spawn(move || serve(&listener, &options));

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a read timeout, so a hang fails rather than blocks");
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("write");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).expect("read");
        reply.lines().next().unwrap_or_default().to_owned()
    }

    #[test]
    fn health_is_ready_once_ready_after_has_passed_and_loading_before() {
        assert_eq!(health_from(&[]), "HTTP/1.1 200 OK", "no wait asked for");
        assert_eq!(
            health_from(&["--ready-after", "60000"]),
            "HTTP/1.1 503 Service Unavailable",
            "a minute's load not yet over"
        );
    }
}
