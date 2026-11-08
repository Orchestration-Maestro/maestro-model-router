//! An item a door offers serves more than one caller.
//!
//! A `mod.rs` door offers its modules and re-exports to the rest of the
//! crate. When exactly one module outside the door uses an offer, and no
//! module inside it shares the offer with the one that defines it, the offer
//! could live next to its caller: the seam holds a single caller and costs a
//! name everyone has to learn. The crate root composes the crate and is not
//! counted as a caller.
//!
//! A seam that stays, for a reason, is recorded in `maestro-quality.toml`.

use crate::rust_source::{Exceptions, Finding, ROOTS, Sources, Tree, repository, unsettled};

/// The rule's identifier in `maestro-quality.toml`.
const RULE: &str = "ARC-005";

/// Every offer in the tree that exactly one outside module uses.
fn one_caller_seams(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for door in tree.doors() {
        for offer in tree.offers(door) {
            let callers = tree.callers(door, &offer);
            if let [only] = callers.outside.as_slice()
                && callers.inside.is_empty()
            {
                found.push(Finding {
                    rule: RULE,
                    path: door.file.clone(),
                    line: offer.line,
                    item: offer.name.clone(),
                    message: format!(
                        "`{}` serves only {}; move it next to its caller, or record in \
                         maestro-quality.toml why this seam stays",
                        offer.name, only.file
                    ),
                });
            }
        }
    }
    found
}

#[test]
fn no_door_offers_what_one_module_alone_uses() {
    let sources = repository();
    let mut found = Vec::new();
    for root in ROOTS {
        found.extend(one_caller_seams(&Tree::load(&sources, root)));
    }
    let failures = unsettled(RULE, &found, &Exceptions::repository());
    assert!(
        failures.is_empty(),
        "A door offers something one module alone uses; move it next to that module, or record \
         in maestro-quality.toml why the seam stays:\n  {}\n",
        failures.join("\n  ")
    );
}

/// The findings of a crate whose files are `pairs`, as `door:line name caller`.
fn seams(pairs: &[(&str, &str)]) -> Vec<String> {
    let sources: Sources = pairs
        .iter()
        .map(|&(path, text)| (path.to_owned(), text.to_owned()))
        .collect();
    one_caller_seams(&Tree::load(&sources, "src/main.rs"))
        .iter()
        .map(|found| format!("{}:{} {}", found.path, found.line, found.item))
        .collect()
}

/// The findings of a crate whose `runner` door offers `only`, which `first`
/// uses and `second` is given as its source.
fn runner_seams(second: &str) -> Vec<String> {
    seams(&[
        ("src/main.rs", "mod first;\nmod runner;\nmod second;\n"),
        ("src/first.rs", "use crate::runner::only;\n"),
        ("src/second.rs", second),
        (
            "src/runner/mod.rs",
            "mod commands;\npub(crate) use commands::only;\n",
        ),
        ("src/runner/commands.rs", "pub(crate) fn only() {}\n"),
    ])
}

#[test]
fn a_re_export_one_outside_module_uses_is_refused() {
    assert_eq!(
        runner_seams("fn nothing() {}\n"),
        ["src/runner/mod.rs:2 only"]
    );
}

#[test]
fn a_re_export_two_outside_modules_use_is_kept() {
    assert_eq!(
        runner_seams("use crate::runner::only;\n"),
        Vec::<String>::new()
    );
}

#[test]
fn a_use_through_an_alias_of_the_door_counts_as_a_caller() {
    let second = "use crate::runner as jobs;\nfn go() { jobs::only(); }\n";
    assert_eq!(runner_seams(second), Vec::<String>::new());
}

#[test]
fn a_glob_import_of_the_door_counts_as_a_caller() {
    assert_eq!(
        runner_seams("use crate::runner::*;\n"),
        Vec::<String>::new()
    );
}

#[test]
fn a_use_in_the_test_module_of_another_file_is_not_a_caller() {
    let second = "fn live() {}\n#[cfg(test)]\nmod tests {\n    use crate::runner::only;\n}\n";
    assert_eq!(runner_seams(second), ["src/runner/mod.rs:2 only"]);
}

#[test]
fn a_module_offered_to_one_outside_module_is_refused() {
    let found = seams(&[
        ("src/main.rs", "mod first;\nmod runner;\n"),
        (
            "src/first.rs",
            "use crate::runner::jobs;\nfn run() { jobs::start(); }\n",
        ),
        ("src/runner/mod.rs", "pub(crate) mod jobs;\n"),
        ("src/runner/jobs.rs", "pub(crate) fn start() {}\n"),
    ]);
    assert_eq!(found, ["src/runner/mod.rs:1 jobs"]);
}

#[test]
fn a_sibling_inside_the_door_that_shares_the_offer_makes_it_a_seam() {
    let found = seams(&[
        ("src/main.rs", "mod first;\nmod runner;\n"),
        ("src/first.rs", "use crate::runner::only;\n"),
        (
            "src/runner/mod.rs",
            "mod commands;\nmod jobs;\npub(crate) use commands::only;\n",
        ),
        ("src/runner/commands.rs", "pub(crate) fn only() {}\n"),
        ("src/runner/jobs.rs", "use super::commands::only;\n"),
    ]);
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn the_crate_root_is_not_a_caller() {
    let found = seams(&[
        (
            "src/main.rs",
            "mod runner;\nfn main() { runner::root_only(); }\n",
        ),
        (
            "src/runner/mod.rs",
            "mod commands;\npub(crate) use commands::root_only;\n",
        ),
        ("src/runner/commands.rs", "pub(crate) fn root_only() {}\n"),
    ]);
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn an_offer_visible_only_to_the_parent_is_not_an_offer() {
    let found = seams(&[
        ("src/main.rs", "mod outer;\n"),
        ("src/outer/mod.rs", "mod user;\nmod runner;\n"),
        ("src/outer/user.rs", "use super::runner::narrow;\n"),
        (
            "src/outer/runner/mod.rs",
            "mod commands;\npub(super) use commands::narrow;\n",
        ),
        (
            "src/outer/runner/commands.rs",
            "pub(crate) fn narrow() {}\n",
        ),
    ]);
    assert_eq!(found, Vec::<String>::new());
}
