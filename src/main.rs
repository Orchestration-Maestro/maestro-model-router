//! The `model-router` binary: check a catalog, measure its entries, launch
//! one, or serve them all, and say which build is doing it.
//!
//! Argument handling is hand-written. A handful of subcommands taking two or
//! three operands do not earn a dependency, and the dependency would have to
//! be justified to the same gates as a real one.
//!
//! This file only hands the command line to [`arguments`], which is the
//! argument table; each command lives in a module of its own.

use std::env;
use std::process::ExitCode;

mod arguments;
mod check;
mod launching;
mod serving;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    arguments::run(&args)
}
