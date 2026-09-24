//! No-machine-paths gate.
//!
//! A path that names one machine -- a home directory, a user profile, a drive
//! letter -- makes a file correct on exactly that machine. Paths are derived at
//! run time instead, and a test uses a synthetic root such as `/somewhere`.
//! Platform roots (`/usr`, `/opt`, `/etc`, `/var`, `/tmp`) name a platform
//! rather than a machine, and are allowed.
//!
//! This ran in the old estate's shared workflow, which no longer exists; it
//! lives here now so the rule in `AGENTS.md` keeps a check behind it.

use std::fs;

mod common;
use common::{has_extension, repo_root, sources};

const SCANNED: &[&str] = &["rs", "md", "toml", "json", "yaml", "yml", "sh"];

/// Files scanned by name, because they carry no extension.
const SCANNED_BY_NAME: &[&str] = &["justfile"];

/// Prefixes that name one machine's home or profile directory, assembled from
/// parts so this file does not match itself.
fn machine_prefixes() -> [String; 3] {
    [
        ["/", "home", "/"].concat(),
        ["/", "Users", "/"].concat(),
        [":", "\\", "Users", "\\"].concat(),
    ]
}

/// Whether `prefix` begins a path somewhere in `line`, rather than sitting
/// inside one: `/somewhere/home/models` is a synthetic root, not a home.
fn starts_a_path(line: &str, prefix: &str) -> bool {
    line.match_indices(prefix).any(|(at, _)| {
        line[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_alphanumeric() || "._-~".contains(before)))
    })
}

/// A Windows drive root -- a letter, a colon and a slash -- which only one
/// machine's disk layout makes true. The letter must start a word, so
/// `Foo::Bar` and a URL scheme are not drives.
fn names_a_drive(line: &str) -> bool {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(2)).any(|at| {
        let starts_a_word = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        starts_a_word
            && bytes[at].is_ascii_uppercase()
            && bytes[at + 1] == b':'
            && matches!(bytes[at + 2], b'\\' | b'/')
    })
}

fn names_a_machine(line: &str) -> bool {
    machine_prefixes()
        .iter()
        .any(|prefix| starts_a_path(line, prefix))
        || names_a_drive(line)
}

#[test]
fn no_file_names_a_machine() {
    let root = repo_root();
    let files: Vec<_> = sources()
        .into_iter()
        .filter(|path| {
            has_extension(path, SCANNED)
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| SCANNED_BY_NAME.contains(&name))
        })
        .collect();
    assert!(!files.is_empty(), "nothing was scanned");

    let mut found = Vec::new();
    for path in &files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if names_a_machine(line) {
                let relative = path.strip_prefix(&root).unwrap_or(path);
                found.push(format!(
                    "  {}:{}: {}",
                    relative.display(),
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "Paths that name one machine; derive them at run time instead:\n\n{}\n",
        found.join("\n")
    );
}

#[test]
fn a_drive_is_a_letter_starting_a_word_before_a_colon_and_a_slash() {
    assert!(names_a_drive(&format!("see {}:\\Windows", 'C')));
    assert!(names_a_drive(&format!("{}:/models", 'D')));
    assert!(!names_a_drive("maestro_model_router::proxy::Router"));
    assert!(!names_a_drive("https://example.org"));
    assert!(!names_a_drive("HTTPS://example.org"));
}

#[test]
fn a_home_prefix_counts_only_where_it_starts_a_path() {
    let home = ["/", "home", "/"].concat();
    assert!(starts_a_path(&format!("root = \"{home}someone\""), &home));
    assert!(starts_a_path(&format!("{home}someone"), &home));
    assert!(!starts_a_path(&format!("/somewhere{home}models"), &home));
}
