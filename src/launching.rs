//! The `launch` command, and what every command that starts a child needs:
//! the catalog it was handed, read, and somewhere for the child's lines to go.

use std::fs;
use std::path::Path;
use std::time::Instant;

use maestro_model_router::catalog::Catalog;
use maestro_model_router::launch::{LineSink, Server, models_root};

/// Every line a child writes, passed on to this process's own standard
/// error, prefixed with the entry it came from, so a service manager's
/// journal keeps it.
pub(crate) fn to_stderr() -> LineSink {
    LineSink::new(|id, line| eprintln!("{id}: {line}"))
}

/// One usable catalog, or why it is not.
pub(crate) fn read(catalog: &Path) -> Result<Catalog, String> {
    let text = fs::read_to_string(catalog)
        .map_err(|error| format!("cannot read {}: {error}", catalog.display()))?;
    Catalog::parse(&text)
        .map_err(|report| format!("{} is not usable:\n{report}", catalog.display()))
}

/// Starts one entry, proves it answers, and stops it again.
///
/// Deliberately not long-running: proving that a child starts, answers and
/// ends is the whole of what this command claims, and it doubles as the manual
/// check against a real `llama-server`. Serving is `serve`'s job, and what
/// that does about signals is recorded there.
pub(crate) fn launch(catalog: &Path, id: &str) -> Result<(), String> {
    let parsed = read(catalog)?;
    let entry = parsed
        .entry(id)
        .ok_or_else(|| format!("{} carries no model called '{id}'", catalog.display()))?;

    let root = models_root().map_err(|failure| failure.to_string())?;
    let server = Server::located(None)
        .map_err(|failure| failure.to_string())?
        .with_sink(to_stderr());

    let started = Instant::now();
    let mut child = server
        .start(entry, &root)
        .map_err(|failure| failure.to_string())?;
    println!(
        "{id} is ready at http://{} after {:.1} seconds",
        child.endpoint(),
        started.elapsed().as_secs_f64()
    );

    child.stop();
    println!("{id} stopped");
    Ok(())
}
