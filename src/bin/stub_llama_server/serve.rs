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
