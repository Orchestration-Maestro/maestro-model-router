//! The argument table: which command a command line names, and how what it
//! had to say becomes the process's exit code.

use std::io;
use std::path::Path;
use std::process::ExitCode;

use maestro_model_router::{bench, build};

use crate::{check, launching, serving};

const USAGE: &str = "usage: model-router check <catalog>\n       \
                     model-router bench <catalog> [model]\n       \
                     model-router launch <catalog> <model>\n       \
                     model-router serve <catalog> [address[,address...]]\n       \
                     model-router --version";

/// Runs the command `args` name, the program's own name left out.
pub(crate) fn run(args: &[String]) -> ExitCode {
    match args {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("{}", build::described());
            ExitCode::SUCCESS
        }
        [command, catalog] if command == "check" => check::check(Path::new(catalog)),
        // Whole catalog or one entry. `check` reads the files and reasons; this
        // starts each entry and reads the card, which is the only way to tell
        // an estimate that is merely arithmetic from one that is true.
        [command, catalog] if command == "bench" => report(bench::command(
            Path::new(catalog),
            None,
            launching::to_stderr(),
            &mut io::stdout(),
        )),
        [command, catalog, id] if command == "bench" => report(bench::command(
            Path::new(catalog),
            Some(id),
            launching::to_stderr(),
            &mut io::stdout(),
        )),
        [command, catalog, id] if command == "launch" => {
            report(launching::launch(Path::new(catalog), id))
        }
        [command, catalog] if command == "serve" => {
            report(serving::serve(Path::new(catalog), None))
        }
        [command, catalog, address] if command == "serve" => {
            report(serving::serve(Path::new(catalog), Some(address)))
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Whatever a command had to say when it could not do its work.
fn report(outcome: Result<(), String>) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(complaint) => {
            eprintln!("{complaint}");
            ExitCode::FAILURE
        }
    }
}
