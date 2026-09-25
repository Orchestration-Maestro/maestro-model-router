//! The `serve` command: every entry in a catalog, served until the process is
//! asked to end, and the children ended with it.

use std::io::{self, Write as _};
use std::net::SocketAddr;
use std::panic;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use maestro_model_router::admission::Budget;
use maestro_model_router::build;
use maestro_model_router::idle::{IdleWindow, Limits};
use maestro_model_router::launch::{Server, models_root};
use maestro_model_router::proxy::{self, ASSIGNED_WITHIN, Access, Router, Source, Voice};
use maestro_model_router::queue::Wait;
use maestro_model_router::startup;

use crate::launching::{read, to_stderr};

/// The one public port the design names, on the interface every machine has.
const DEFAULT_ADDRESS: &str = "127.0.0.1:8080";

/// Serves every entry in the catalog, on its own endpoint and on the shared
/// one.
///
/// Long-running by design, unlike `launch`: now there is something to serve.
/// It runs until the process is asked to end, and that end is a signal, so a
/// signal is what reaches `Router::stop`: see [`until_ended`]. What no handler
/// can cover, and how to find the children afterwards, is in `README.md`
/// under what eviction never does.
pub(crate) fn serve(catalog: &Path, address: Option<&str>) -> Result<(), String> {
    let parsed = read(catalog)?;
    let root = models_root().map_err(|failure| failure.to_string())?;
    let server = Server::located(None)
        .map_err(|failure| failure.to_string())?
        .with_sink(to_stderr());

    let wanted = addresses(address.unwrap_or(DEFAULT_ADDRESS))?;

    let budget = Budget::configured().map_err(|failure| failure.to_string())?;
    let idle_window = IdleWindow::configured().map_err(|failure| failure.to_string())?;
    // Composed before the catalog is handed over, because binding takes it.
    let limit_mib = budget.limit_mib();
    let budget_source = budget.source().to_owned();
    let idle_seconds = idle_window.seconds();
    let reserved_mib = parsed.resident_reservation_mib();

    let wait = Wait::configured().map_err(|failure| failure.to_string())?;
    let waiting = wait.waits();
    let access = Access::configured();
    let access_rules = access.described();
    let limits = Limits::new(budget, idle_window, wait)
        .with_access(access)
        .with_voice(to_stdout_and_stderr());
    // The path travels with what was parsed from it: `POST /reload` reads the
    // same file again, and a router handed only the parsed value would have
    // nowhere to read it from.
    let source = Source {
        catalog: parsed,
        path: catalog.to_path_buf(),
    };
    proxy::await_assigned(&wanted, ASSIGNED_WITHIN, &mut io::stderr());
    let router = Router::bind(&wanted, source, root, server, limits)
        .map_err(|failure| failure.to_string())?;
    let router = Arc::new(router);
    for address in router.addresses() {
        println!("serving on http://{address}");
    }
    // The paths are the same on every address, so they are shown once, under
    // the first: repeating them per address would say the router routes
    // differently depending on where a request arrived, which it does not.
    let bound = router.address();
    println!("  http://{bound}/models/<model>/v1/chat/completions");
    println!("  http://{bound}/v1/chat/completions   (routed by the body's model)");
    println!("  POST http://{bound}/reload                (re-reads the catalog)");
    println!("{access_rules}");
    println!("{}", startup::budget(limit_mib, &budget_source));
    println!("{}", startup::admission_wait(waiting));
    if reserved_mib > 0 {
        println!("{}", startup::reservation(limit_mib, reserved_mib));
    }
    println!("{}", startup::idle_window(idle_seconds));
    println!("a streamed reply is passed through as it arrives");
    println!("{}", build::described());

    until_ended(&router)
}

/// The router's lines for its operator, on this process's own streams: what
/// it did on standard output, a resident that would not load on standard
/// error.
///
/// Written rather than printed, with the error dropped: a closed standard
/// output is not a reason to stop serving, and `println!` panics on one --
/// which would end whichever thread said the line, and a reaper thread that
/// ends is idle unloading that silently stops.
fn to_stdout_and_stderr() -> Voice {
    Voice::new(
        |line| drop(writeln!(io::stdout(), "{line}")),
        |line| drop(writeln!(io::stderr(), "{line}")),
    )
}

/// Every address one operand names, separated by commas.
///
/// A list rather than one address because a single router serves every
/// interface its operator named -- the machine it runs on, a bridge to a lab
/// -- and each may carry its own port. Commas rather than a repeated flag
/// because argument handling here is hand-written, and a flag parser is the
/// dependency this binary does without.
///
/// An operand that names nothing at all yields nothing, which `Router::bind`
/// refuses by name rather than accepting as a router with no way in.
fn addresses(operand: &str) -> Result<Vec<SocketAddr>, String> {
    operand
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            piece
                .parse()
                .map_err(|error| format!("'{piece}' is not an address to bind: {error}"))
        })
        .collect()
}

/// What the thread waiting for the end of the process hears.
enum Ending {
    /// The process was asked to end.
    Signalled,
    /// The children were ended after the first signal.
    Stopped,
    /// `Router::serve` returned, or its thread panicked.
    Served,
}

/// Tells the waiting thread that serving is over, however it ended.
///
/// Sent from `drop` so a panic on the serving thread reaches the waiting
/// thread too, instead of leaving it waiting for a signal.
struct Served(Sender<Ending>);

impl Drop for Served {
    fn drop(&mut self) {
        drop(self.0.send(Ending::Served));
    }
}

/// Serves on a thread of its own and ends the children when the process is
/// asked to end, then returns so the process can.
///
/// A child is a separate process that nothing in the operating system ties to
/// this one, and `Router::serve` never returns. Without this, `SIGTERM` --
/// what `systemctl stop`, `kill` and a container stop all send -- ended the
/// router and left every server it had started running with its memory. The
/// handler covers the ways a process is asked to end: `SIGTERM`, `SIGINT` and
/// `SIGHUP` on Unix; Ctrl-C, Ctrl-Break and a closing console on Windows. What
/// it cannot cover is being killed outright -- `SIGKILL`, `taskkill /F` --
/// which no process is allowed to handle.
///
/// The handler only reports the signal, and this thread decides: the process
/// ends when `main` returns, so the thread that returns to it is the one that
/// waits, and serving moves to a thread of its own. The stop runs on a third
/// thread so that a second signal is acted on rather than queued: it ends the
/// process immediately, with a failing status because the children were not
/// waited for. Stopping on this thread would make the second signal wait for
/// the first, and a stop held up by a child that will not die would then be a
/// router nothing but `SIGKILL` can end -- the state this exists to remove.
fn until_ended(router: &Arc<Router>) -> Result<(), String> {
    let (ending, endings) = mpsc::channel();
    // Registered after the bind and before anything can start a child: a
    // signal before this point ends a router that has nothing to stop.
    let signalled = ending.clone();
    ctrlc::set_handler(move || drop(signalled.send(Ending::Signalled)))
        .map_err(|error| format!("cannot handle termination signals: {error}"))?;

    let serving = Arc::clone(router);
    let told = Served(ending.clone());
    let served = thread::spawn(move || {
        let _told = told;
        serving.serve();
    });

    if let Ending::Served = wait_for_the_end(router, &ending, &endings)? {
        // A serving thread that panicked panics here, as it did when it
        // served on this one: the same message, already printed, and the
        // same status.
        if let Err(payload) = served.join() {
            panic::resume_unwind(payload);
        }
    }
    Ok(())
}

/// Waits for whatever ends the process: the stop a first signal starts, a
/// second signal, which is the complaint, or serving that ended by itself.
fn wait_for_the_end(
    router: &Arc<Router>,
    ending: &Sender<Ending>,
    endings: &Receiver<Ending>,
) -> Result<Ending, String> {
    let mut signalled = false;
    loop {
        match endings.recv() {
            Ok(Ending::Signalled) if signalled => {
                return Err("signalled again: exiting without waiting for the children".to_owned());
            }
            Ok(Ending::Signalled) => {
                signalled = true;
                let stopping = Arc::clone(router);
                let stopped = ending.clone();
                thread::spawn(move || {
                    let held = stopping.loaded().len();
                    stopping.stop();
                    println!("stopping: ended {}", children(held));
                    drop(stopped.send(Ending::Stopped));
                });
            }
            Ok(Ending::Stopped) => return Ok(Ending::Stopped),
            Ok(Ending::Served) | Err(_) => return Ok(Ending::Served),
        }
    }
}

/// `1 child`, `2 children`, so the stop line reads as a sentence.
fn children(count: usize) -> String {
    if count == 1 {
        "1 child".to_owned()
    } else {
        format!("{count} children")
    }
}
