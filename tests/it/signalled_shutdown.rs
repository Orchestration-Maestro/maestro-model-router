//! A signalled router ends its children before it goes.
//!
//! `SIGTERM` is what `systemctl stop`, a container stop and a plain `kill`
//! send, and a child is a separate process that nothing in the operating
//! system ends when its parent does. So this drives the real binary rather
//! than the library: the handler that turns a signal into `Router::stop` lives
//! in `main`, and only a process can be signalled.
//!
//! Unix only, and not for want of trying. There is no portable way to send a
//! console control event to another process from a test: `GenerateConsoleCtrlEvent`
//! reaches every process attached to the test's own console, this one
//! included, and a process started with its own console cannot be reached at
//! all. The Windows leg still compiles this crate to nothing and runs the rest.
#![cfg(unix)]

use std::fs;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::support::spawned::{RouterProcess, SearchPath};
use crate::support::{MODEL, ModelsRoot, catalog_text, get, health, request, status};

/// Polls until the child's port stops answering, or fails saying it did not.
fn assert_goes_quiet(endpoint: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if health(endpoint).is_none() {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    panic!(
        "the child at {endpoint} outlived the router that was signalled to \
         stop. This is the process a service manager's stop leaves behind, \
         holding its memory."
    );
}

#[test]
fn a_terminated_router_ends_its_children_and_exits_cleanly() {
    let root = ModelsRoot::with(&[MODEL]);
    let catalog = root.path().join("catalog.toml");
    fs::write(&catalog, catalog_text("")).expect("a catalog in the temporary directory");
    let search = SearchPath::with_stub();

    let mut router = RouterProcess::serve(&catalog, &root, &search);
    let address = router.address();

    // One request, so a child exists to be orphaned.
    let reply = request(address, &get("/models/gemma3/v1/echo"));
    assert_eq!(status(&reply), Some(200), "a child answered:\n{reply}");
    // The stub reflects the Host it was given, which the rewrite set to the
    // child's own address. That is how this test learns a port the router
    // never told anyone about.
    let endpoint = reply
        .lines()
        .find_map(|line| line.strip_prefix("Host: "))
        .expect("the echo carries the address the child was reached on")
        .to_owned();
    assert_eq!(
        health(endpoint.as_str()),
        Some(200),
        "the child answers while the router holds it"
    );

    router.terminate();

    let exit = router
        .exited_within(Duration::from_secs(20))
        .unwrap_or_else(|| {
            panic!(
                "the router was still running 20 seconds after SIGTERM; stderr said:\n{}",
                router.stderr()
            )
        });
    assert_eq!(
        exit.code(),
        Some(0),
        "a signalled router stops its children and exits cleanly. No code at \
         all means the signal's default action ended it before it could stop \
         anything: {exit}"
    );
    assert_goes_quiet(&endpoint);

    let said = router.rest_of_stdout();
    assert!(
        said.contains("ended 1 child"),
        "the router says how many children it ended on the way out:\n{said}"
    );
}
