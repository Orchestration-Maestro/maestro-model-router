//! Every Rust file belongs to a build target the conventions checks read.
//!
//! The checks start from each target's root file and follow its `mod`
//! declarations. A file no declaration reaches, or a target missing from
//! `rust_source::ROOTS`, would be skipped without a word; this check says so.

use std::collections::BTreeSet;

use crate::rust_source::{ROOTS, Sources, Tree, repository};

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
