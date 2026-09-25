//! A stand-in for `llama-server`, so continuous integration can supervise a
//! real process and relay a real stream without a real model.
//!
//! A real server needs a multi-gigabyte model file and a graphics card.
//! Neither exists in continuous integration, so a test that requires one is a
//! test that never runs. This binary speaks the parts of the server contract
//! the router reads, and the supervision and relay paths around it are
//! identical either way: the router picks a port, builds a command line,
//! spawns, polls, reads a request head, and copies bytes.
//!
//! [`options`] reads what the stub was asked to do, [`serve`] does it, and
//! [`reply`] decides what one connection is answered with.
//!
//! It is never released. The shared release workflow takes the name of one
//! binary, and that binary is `model-router`.

use std::env;
use std::process::ExitCode;

mod options;
mod reply;
mod serve;

fn main() -> ExitCode {
    match options::parse(env::args().skip(1)) {
        Ok(options) => serve::run(options),
        Err(complaint) => {
            eprintln!("stub-llama-server: {complaint}");
            ExitCode::FAILURE
        }
    }
}
