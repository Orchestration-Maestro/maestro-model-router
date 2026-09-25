//! Schema gate for the model catalog.
//!
//! The catalog is the only input slice 1 has, so its shape is the whole
//! contract: what a model entry must carry, what it may omit, and what it
//! inherits. These tests fix that contract before a parser exists.
//!
//! Two rules earn their own cases. Every validation error names both the
//! entry it came from and the field that caused it, because an error that
//! says only "invalid catalog" sends the reader back to the file to guess.
//! And a path anchored to one machine cannot be constructed at all -- the
//! shared gate scans tracked files, and this proves the type refuses one at
//! run time too.

use std::fs;
use std::path::Path;

use maestro_model_router::catalog::{Catalog, RelativePath, Residency};

/// The golden catalog, parsed. A fixture rather than an inline string: it is
/// the same shape the shipped catalog uses, and a file can be read by eye.
fn golden() -> Catalog {
    let text = include_str!("../fixtures/catalog.toml");
    Catalog::parse(text).expect("the golden fixture must parse")
}

#[test]
fn the_resident_reservation_is_what_the_resident_entries_cost() {
    assert_eq!(
        golden().resident_reservation_mib(),
        1024,
        "one resident at 1024 MiB. An on-demand entry costs nothing here \
         however large it is, because a reservation is what stays held \
         whatever else the router does"
    );
}

#[test]
fn the_golden_catalog_parses_field_by_field() {
    let catalog = golden();
    assert_eq!(catalog.version, 1);
    assert_eq!(catalog.entries.len(), 4, "four entries, one per model");

    let qwen = catalog.entry("qwen38").expect("qwen38");
    assert_eq!(
        qwen.path.as_str(),
        "llm/qwen/qwen3.8-27b/Qwen3.8-27B-UD-Q6_K.gguf"
    );
    assert_eq!(
        qwen.draft_path.as_ref().map(RelativePath::as_str),
        Some("llm/qwen/qwen3.8-27b/MTP/mtp-Qwen3.8-27B-Q4_0.gguf"),
        "the speculative draft model"
    );
    assert_eq!(
        qwen.projector_path.as_ref().map(RelativePath::as_str),
        Some("llm/qwen/qwen3.8-27b/mmproj-F16.gguf"),
        "the multimodal projector"
    );
    assert_eq!(qwen.context_size, 131_072);
    assert_eq!(qwen.memory_estimate_mib, 24_576);
    assert_eq!(qwen.reasoning_format.as_deref(), Some("deepseek"));
    assert_eq!(
        qwen.reasoning_effort, None,
        "only the semantic entry sets it"
    );
}

#[test]
fn an_entry_inherits_every_default_it_does_not_set() {
    let catalog = golden();

    let gemma = catalog.entry("gemma3").expect("gemma3");
    assert_eq!(gemma.context_size, 32_768, "inherited from defaults");
    assert_eq!(gemma.residency, Residency::OnDemand, "inherited");
    assert_eq!(gemma.memory_estimate_mib, 2_048, "set by the entry");
    assert_eq!(gemma.draft_path, None, "no draft model");
    assert_eq!(gemma.projector_path, None, "no projector");

    let qwen = catalog.entry("qwen38").expect("qwen38");
    assert_eq!(
        qwen.context_size, 131_072,
        "the entry overrides the default"
    );

    // Startup time varies by two orders of magnitude, so the budget is a per
    // entry field: the small model carries a tight one of its own, the large
    // one inherits the generous default.
    assert_eq!(
        gemma.startup_timeout_seconds, 60,
        "a small model is ready quickly, and the catalog says so"
    );
    assert_eq!(
        qwen.startup_timeout_seconds, 300,
        "inherited from the defaults table"
    );

    assert_eq!(
        qwen.flags.get("jinja").map(String::as_str),
        Some("true"),
        "flags merge rather than replace"
    );
    assert_eq!(
        qwen.flags.get("ctk").map(String::as_str),
        Some("q8_0"),
        "the entry's own flags survive the merge"
    );
}

#[test]
fn residency_is_parsed_and_only_one_entry_is_resident() {
    let catalog = golden();
    assert_eq!(
        catalog.entry("qwen3-06b").expect("qwen3-06b").residency,
        Residency::Resident,
        "the entry the steward depends on"
    );
    assert_eq!(
        catalog
            .entry("qwen38-semantic")
            .expect("qwen38-semantic")
            .reasoning_effort
            .as_deref(),
        Some("low"),
    );
}

/// One case per validation rule. Table-driven so the six read as one list of
/// rules rather than six near-identical functions.
const INVALID: &[(&str, &str, &str, &str)] = &[
    (
        "a required field is missing",
        "version = 1\n[models.alpha]\ncontext_size = 4096\nmemory_estimate_mib = 512\n",
        "alpha",
        "path",
    ),
    (
        "a field nobody recognises",
        "version = 1\n[models.beta]\npath = \"a.gguf\"\ncontext_size = 4096\n\
         memory_estimate_mib = 512\ncolour = \"red\"\n",
        "beta",
        "colour",
    ),
    (
        "a context size of zero",
        "version = 1\n[models.gamma]\npath = \"a.gguf\"\ncontext_size = 0\n\
         memory_estimate_mib = 512\n",
        "gamma",
        "context_size",
    ),
    (
        "a residency nobody recognises",
        "version = 1\n[models.delta]\npath = \"a.gguf\"\ncontext_size = 4096\n\
         memory_estimate_mib = 512\nresidency = \"sometimes\"\n",
        "delta",
        "residency",
    ),
    (
        "a memory estimate of zero",
        "version = 1\n[models.epsilon]\npath = \"a.gguf\"\ncontext_size = 4096\n\
         memory_estimate_mib = 0\n",
        "epsilon",
        "memory_estimate_mib",
    ),
    (
        "a path anchored to a machine",
        "version = 1\n[models.zeta]\npath = \"/somewhere/a.gguf\"\ncontext_size = 4096\n\
         memory_estimate_mib = 512\n",
        "zeta",
        "path",
    ),
];

#[test]
fn every_validation_error_names_its_entry_and_its_field() {
    for (case, text, entry, field) in INVALID {
        let report = Catalog::parse(text)
            .err()
            .unwrap_or_else(|| panic!("{case}: the catalog must be refused"))
            .to_string();
        assert!(
            report.contains(entry),
            "{case}: the error must name the entry '{entry}':\n{report}"
        );
        assert!(
            report.contains(field),
            "{case}: the error must name the field '{field}':\n{report}"
        );
    }
}

#[test]
fn every_error_is_reported_not_only_the_first() {
    let text = "version = 1\n\
                [models.alpha]\ncontext_size = 0\nmemory_estimate_mib = 512\n\
                [models.beta]\npath = \"b.gguf\"\ncontext_size = 4096\nmemory_estimate_mib = 0\n";
    let report = Catalog::parse(text)
        .expect_err("both entries are invalid")
        .to_string();
    assert!(report.contains("alpha"), "the first entry:\n{report}");
    assert!(report.contains("beta"), "the second entry too:\n{report}");
}

/// One bad entry can be wrong in several ways at once, and a reader fixing it
/// should see all of them before running the tool again.
#[test]
fn one_entry_reports_all_of_its_own_faults() {
    let text = "version = 1\n\
                [models.alpha]\ncontext_size = 0\ncolour = \"red\"\n";
    let report = Catalog::parse(text)
        .expect_err("the entry is invalid three times over")
        .to_string();
    for expected in ["path", "context_size", "memory_estimate_mib", "colour"] {
        assert!(
            report.contains(expected),
            "'{expected}' must be reported alongside the others:\n{report}"
        );
    }
}

/// The card this catalog is measured against, and the budget it is written
/// for: the whole of it.
///
/// `Budget::derived` holds a tenth of a device back, which is the right default
/// for a machine nobody has measured. This one has been measured entry by
/// entry with `model-router bench`, and the tenth was costing it real context:
/// turbo38 needs 29745 MiB and a tenth-held budget is 29347, so a 27B at its
/// trained window was refused over 398 MiB on a card with 32,607. The service
/// sets `MAESTRO_MEMORY_BUDGET_MIB` to the total, and this test asserts
/// against the same figure the estate runs with rather than a default it
/// overrides.
///
/// What stops a load running the card out is not this ceiling in any case.
/// Admission re-reads what the device reports free immediately before starting
/// a child, so the live figure is the guard; the budget is a declared ceiling
/// for planning, and planning against a tenth that is never used is planning
/// against fiction.
const CARD_MIB: u64 = 32_607;

/// The catalog that ships with this repository, parsed.
fn shipped() -> Catalog {
    let shipped = concat!(env!("CARGO_MANIFEST_DIR"), "/catalog.toml");
    let text = fs::read_to_string(shipped).expect("catalog.toml ships with this repository");
    Catalog::parse(&text).unwrap_or_else(|report| {
        panic!("the shipped catalog must be valid:\n{report}");
    })
}

/// A model caught in a repetition loop generates until its context is full --
/// 196,608 tokens for the largest entry, over an hour of a card nobody else
/// can use -- because `llama-server` caps nothing unless told to. Every entry
/// that generates text carries a cap, so a loop ends on its own; a client that
/// wants a longer answer asks for it with `max_tokens`.
#[test]
fn every_generating_entry_of_the_shipped_catalog_caps_its_answer() {
    let catalog = shipped();
    let uncapped: Vec<&str> = catalog
        .entries
        .iter()
        .filter(|entry| entry.generates())
        .filter(|entry| {
            !["n-predict", "predict", "n"].iter().any(|flag| {
                entry
                    .flags
                    .get(*flag)
                    .is_some_and(|value| value.trim().parse::<u32>().is_ok_and(|cap| cap > 0))
            })
        })
        .map(|entry| entry.id.as_str())
        .collect();
    assert!(
        uncapped.is_empty(),
        "these entries generate with no cap on an answer's length: {uncapped:?}"
    );
}

/// The file that ships cannot rot away from the parser that reads it.
#[test]
fn the_shipped_catalog_is_valid() {
    let catalog = shipped();
    // What a resident reserves is never evicted, so the largest entry has to
    // fit in what is left *after* the reservation -- not merely inside the
    // budget. This catalog once held a 1 GiB steward resident beside a 29,184
    // MiB flagship, and the flagship could never load: not after a wait, not
    // with every other model unloaded. Nothing in the shape of the file said
    // so, and the refusal named whichever models happened to be loaded, which
    // reads like a clash that clears in a moment.
    //
    // The card is named here because this catalog is written for this machine
    // and the repository says so; a catalog that moves to another card is
    // expected to fail this and be re-measured, which is the coupling working
    // rather than the test being brittle.
    let budget = CARD_MIB;
    let reservation = catalog.resident_reservation_mib();
    let largest = catalog
        .entries
        .iter()
        .map(|entry| u64::from(entry.memory_estimate_mib))
        .max()
        .expect("the shipped catalog has entries");

    assert!(
        largest + reservation <= budget,
        "the largest entry needs {largest} MiB and the resident entries hold \
         {reservation} MiB of the {budget} MiB budget back for good, so it \
         could never load. Either make a resident on-demand, or bring the \
         largest entry's estimate or context down."
    );
}

#[test]
fn a_machine_anchored_path_cannot_be_represented() {
    // The drive is assembled from its letter so this file stays clear of the
    // no-machine-paths gate while still handing the constructor a real one.
    let drive = format!("{}:\\models\\a.gguf", 'C');
    for anchored in [
        "/somewhere/models/a.gguf",
        drive.as_str(),
        "\\\\server\\share\\a.gguf",
    ] {
        assert!(
            RelativePath::new(anchored).is_err(),
            "the constructor must refuse '{anchored}'"
        );
    }
}

#[test]
fn a_relative_path_resolves_against_a_models_root() {
    let path = RelativePath::new("llm/qwen/a.gguf").expect("a relative path is accepted");
    assert_eq!(path.as_str(), "llm/qwen/a.gguf");
    assert_eq!(
        path.resolve(Path::new("/somewhere/models")),
        Path::new("/somewhere/models").join("llm/qwen/a.gguf"),
        "resolution is the caller's decision, not the catalog's"
    );
}
