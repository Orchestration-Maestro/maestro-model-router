//! Walking the root for model files, and turning each into an entry.
//!
//! Split from the door above it when `discover.rs` became `discover/mod.rs`,
//! which may only declare. The rules the walk applies are set out there; what
//! a single file name says is `name.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::gguf::Metadata;

use super::super::entry::{Entry, Residency};
use super::super::estimate::{self, Basis};
use super::super::path::RelativePath;
use super::super::read::{DEFAULT_STARTUP_TIMEOUT_SECONDS, Defaults};
use super::name::{self, identifier, is_model};

/// How deep the walk goes. What bounds a link that points at an ancestor.
const MAX_DEPTH: usize = 16;

/// The context a discovered entry gets when neither the defaults table nor
/// the file says: the server's own default.
const FALLBACK_CONTEXT: u32 = 4096;

/// Every model file under `root` that `entries` do not name, as entries.
pub(in crate::catalog) fn under(
    root: &Path,
    entries: &[Entry],
    defaults: &Defaults,
    notes: &mut Vec<String>,
) -> Vec<Entry> {
    let referenced: BTreeSet<PathBuf> = entries
        .iter()
        .flat_map(|entry| {
            [
                Some(&entry.path),
                entry.draft_path.as_ref(),
                entry.projector_path.as_ref(),
            ]
        })
        .flatten()
        .map(|location| location.resolve(root))
        .collect();
    let mut taken: BTreeSet<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    taken.extend(name::RESERVED.iter().map(|id| (*id).to_owned()));

    let mut found = Vec::new();
    let mut files = Vec::new();
    walk(root, 0, &mut files);
    for file in files {
        if referenced.contains(&file) {
            continue;
        }
        let Some(location) = relative(root, &file) else {
            notes.push(format!(
                "skipped '{}': its path is not text this catalog can carry",
                file.display()
            ));
            continue;
        };
        let Some(id) = identifier(&file, &mut taken, notes) else {
            continue;
        };
        if let Some(entry) = entry(id, location, root, defaults, notes) {
            found.push(entry);
        }
    }
    found
}

/// Every model file below `directory`, in a stable order.
fn walk(directory: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(listing) = fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<PathBuf> = listing.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, depth + 1, out);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_model)
        {
            out.push(path);
        }
    }
}

/// The location as a catalog would write it: relative to the root, with
/// the separator every catalog uses.
fn relative(root: &Path, file: &Path) -> Option<RelativePath> {
    let parts: Option<Vec<&str>> = file
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect();
    RelativePath::new(&parts?.join("/")).ok()
}

/// The entry one file becomes, or `None` once the note says why not.
fn entry(
    id: String,
    location: RelativePath,
    root: &Path,
    defaults: &Defaults,
    notes: &mut Vec<String>,
) -> Option<Entry> {
    let trained_for = Metadata::read(&location.resolve(root))
        .ok()
        .and_then(|metadata| metadata.of_model("context_length"))
        .and_then(|context| u32::try_from(context).ok())
        .filter(|context| *context > 0);
    let context_size = defaults
        .context_size
        .or(trained_for)
        .unwrap_or(FALLBACK_CONTEXT)
        .min(trained_for.unwrap_or(u32::MAX));

    let mut entry = Entry {
        id,
        path: location,
        draft_path: None,
        projector_path: None,
        context_size,
        residency: Residency::OnDemand,
        memory_estimate_mib: 0,
        reasoning_format: defaults.reasoning_format.clone(),
        reasoning_effort: defaults.reasoning_effort.clone(),
        startup_timeout_seconds: defaults
            .startup_timeout_seconds
            .unwrap_or(DEFAULT_STARTUP_TIMEOUT_SECONDS),
        // A discovered entry takes the stock server. Which build a model needs
        // is not a thing a file name can say, so it is not a thing discovery
        // may guess at -- a catalog states it or it is not wanted.
        runtime: None,
        flags: defaults.flags.clone(),
    };
    match estimate::derive(&entry, root) {
        Ok(derived) => {
            entry.memory_estimate_mib = derived.mib;
            let basis = match derived.basis {
                Basis::Metadata => "",
                Basis::Size => ", from file size alone",
            };
            notes.push(format!(
                "discovered '{}' at {}: {} MiB estimated at {} tokens of context{basis}",
                entry.id,
                entry.path.as_str(),
                derived.mib,
                entry.context_size
            ));
            Some(entry)
        }
        Err(reason) => {
            notes.push(format!("skipped '{}': {reason}", entry.path.as_str()));
            None
        }
    }
}
