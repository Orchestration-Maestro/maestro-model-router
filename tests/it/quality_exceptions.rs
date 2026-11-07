//! `maestro-quality.toml` is the one reviewed file that excuses a finding of
//! the conventions checks, and it can excuse nothing it does not name.

use crate::rust_source::{Exceptions, Finding, unsettled};

/// One exception for `ARC-005` in `src/a/mod.rs`, naming `kept`.
const EXCEPTION: &str = "[[exception]]\nrule = \"ARC-005\"\npath = \"src/a/mod.rs\"\n\
                         item = \"kept\"\nreason = \"it is a seam on purpose\"\n";

/// A finding of `rule` about `item` of `src/a/mod.rs`.
fn finding(rule: &'static str, item: &str) -> Finding {
    Finding {
        rule,
        path: "src/a/mod.rs".to_owned(),
        line: 3,
        item: item.to_owned(),
        message: "it serves one caller".to_owned(),
    }
}

#[test]
fn a_finding_with_no_exception_is_reported_with_its_place() {
    let exceptions = Exceptions::parse(EXCEPTION).expect("well-formed");
    assert_eq!(
        unsettled("ARC-005", &[finding("ARC-005", "other")], &exceptions),
        [
            "ARC-005 src/a/mod.rs:3: it serves one caller",
            "ARC-005 src/a/mod.rs: the exception for `kept` excuses nothing any more; remove it \
             from maestro-quality.toml"
        ]
    );
}

#[test]
fn a_finding_an_exception_names_is_excused() {
    let exceptions = Exceptions::parse(EXCEPTION).expect("well-formed");
    let found = [finding("ARC-005", "kept")];
    assert_eq!(
        unsettled("ARC-005", &found, &exceptions),
        Vec::<String>::new()
    );
}

#[test]
fn an_exception_for_the_same_item_under_another_rule_excuses_nothing() {
    let exceptions = Exceptions::parse(EXCEPTION).expect("well-formed");
    let found = [finding("TST-001", "kept")];
    assert_eq!(
        unsettled("TST-001", &found, &exceptions),
        ["TST-001 src/a/mod.rs:3: it serves one caller"]
    );
}

#[test]
fn an_exception_for_another_rule_is_not_stale_for_this_one() {
    let exceptions = Exceptions::parse(EXCEPTION).expect("well-formed");
    assert_eq!(unsettled("TST-001", &[], &exceptions), Vec::<String>::new());
}

#[test]
fn an_exception_without_a_reason_is_refused() {
    let text = EXCEPTION.replace("reason = \"it is a seam on purpose\"\n", "reason = \" \"\n");
    assert_eq!(
        Exceptions::parse(&text).expect_err("no reason"),
        "exception 1 has no reason"
    );
}

#[test]
fn an_exception_without_an_item_is_refused() {
    let text = EXCEPTION.replace("item = \"kept\"\n", "");
    assert_eq!(
        Exceptions::parse(&text).expect_err("no item"),
        "exception 1 has no item"
    );
}

#[test]
fn the_exceptions_the_repository_records_are_well_formed() {
    drop(Exceptions::repository());
}
