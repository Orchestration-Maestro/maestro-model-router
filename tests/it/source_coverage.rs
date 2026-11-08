//! Every Rust file belongs to a build target the conventions checks read.
//!
//! The checks start from each target's root file and follow its `mod`
//! declarations. A file no declaration reaches, or a target whose root file
//! `rust_source::ROOTS` does not name, would be skipped without a word; this
//! check says so.

use std::collections::BTreeSet;
use std::fs;

use toml::{Table, Value};

use crate::common::repo_root;
use crate::rust_source::{ROOTS, Sources, Tree, repository};

/// The target kinds cargo finds by name, with the directory it looks in. The
/// library is not among them: it names one file, and that file is `src/lib.rs`
/// unless it says otherwise.
const TARGETS: &[(&str, &str)] = &[
    ("bin", "src/bin"),
    ("test", "tests"),
    ("bench", "benches"),
    ("example", "examples"),
];

/// The files of `sources` under `src/` or `tests/` that no root reaches.
fn unreached(sources: &Sources) -> Vec<String> {
    let reached: BTreeSet<String> = ROOTS
        .iter()
        .flat_map(|root| Tree::load(sources, root).files())
        .collect();
    sources
        .keys()
        .filter(|path| path.starts_with("src/") || path.starts_with("tests/"))
        .filter(|path| !reached.contains(*path))
        .cloned()
        .collect()
}

/// The text `table` gives under `key`, empty when it gives none.
fn text(table: Option<&Table>, key: &str) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The targets of one kind, each as the table `Cargo.toml` declares it in.
fn targets<'a>(manifest: &'a Table, kind: &str) -> Vec<&'a Table> {
    manifest
        .get(kind)
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(Value::as_table).collect())
        .unwrap_or_default()
}

/// The root file of each target `manifest` declares, when that file exists:
/// the `path` the entry gives, or the first place cargo looks without one.
fn declared_roots(manifest: &Table, sources: &Sources) -> Vec<String> {
    let package = text(manifest.get("package").and_then(Value::as_table), "name");
    let library = manifest.get("lib").and_then(Value::as_table);
    let mut declared: Vec<Vec<String>> = Vec::new();
    if library.is_some() {
        let path = text(library, "path");
        let default = if path.is_empty() {
            "src/lib.rs".to_owned()
        } else {
            path
        };
        declared.push(vec![default]);
    }
    for (kind, directory) in TARGETS {
        for entry in targets(manifest, kind) {
            let name = text(Some(entry), "name");
            let path = text(Some(entry), "path");
            declared.push(if !path.is_empty() {
                vec![path]
            } else if *kind == "bin" && name == package {
                vec!["src/main.rs".to_owned()]
            } else {
                vec![
                    format!("{directory}/{name}.rs"),
                    format!("{directory}/{name}/main.rs"),
                ]
            });
        }
    }
    declared
        .into_iter()
        .filter_map(|places| places.into_iter().find(|place| sources.contains_key(place)))
        .collect()
}

/// The root files of the targets `manifest` declares that `ROOTS` does not
/// name, so no check reads their files.
fn unread_targets(manifest: &str, sources: &Sources) -> Vec<String> {
    let table: Table = manifest.parse().expect("the manifest is TOML");
    let unread: BTreeSet<String> = declared_roots(&table, sources)
        .into_iter()
        .filter(|root| !ROOTS.contains(&root.as_str()))
        .collect();
    unread.into_iter().collect()
}

#[test]
fn every_rust_file_belongs_to_a_target_the_checks_read() {
    let sources = repository();
    let missing = unreached(&sources);
    assert!(
        missing.is_empty(),
        "No build target reaches these files; declare them with `mod`, or add their target's \
         root to ROOTS in tests/it/rust_source/files.rs:\n  {}\n",
        missing.join("\n  ")
    );
}

#[test]
fn every_target_the_manifest_declares_is_read() {
    let sources = repository();
    let manifest =
        fs::read_to_string(repo_root().join("Cargo.toml")).expect("Cargo.toml is readable");
    let unread = unread_targets(&manifest, &sources);
    assert!(
        unread.is_empty(),
        "These targets are built from a root file ROOTS does not name, so the checks read none \
         of their files; add each to ROOTS in tests/it/rust_source/files.rs:\n  {}\n",
        unread.join("\n  ")
    );
}

#[test]
fn a_file_no_module_declares_is_reported() {
    let sources: Sources = [
        ("src/main.rs", "mod used;\n"),
        ("src/used.rs", "fn used() {}\n"),
        ("src/forgotten.rs", "fn forgotten() {}\n"),
        ("tests/it/main.rs", "mod lost;\n"),
        ("tests/it/lost.rs", "fn lost() {}\n"),
        ("tests/it/orphan/mod.rs", "fn orphan() {}\n"),
        ("docs/example.rs", "fn not_a_target_file() {}\n"),
    ]
    .iter()
    .map(|&(path, text)| (path.to_owned(), text.to_owned()))
    .collect();
    assert_eq!(
        unreached(&sources),
        ["src/forgotten.rs", "tests/it/orphan/mod.rs"]
    );
}

#[test]
fn a_target_rooted_outside_the_source_tree_is_reported() {
    let sources: Sources = [
        ("src/lib.rs", "fn library() {}\n"),
        ("tools/probe/main.rs", "fn main() {}\n"),
    ]
    .iter()
    .map(|&(path, text)| (path.to_owned(), text.to_owned()))
    .collect();
    let manifest = "[package]\nname = \"probe\"\n\n[lib]\nname = \"probe\"\n\n\
                    [[bin]]\nname = \"probe\"\npath = \"tools/probe/main.rs\"\n";
    assert_eq!(unread_targets(manifest, &sources), ["tools/probe/main.rs"]);
}
