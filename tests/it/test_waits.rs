//! A test never waits on time.
//!
//! A sleep either hides a race or makes the test slow on every machine and
//! flaky on a loaded one. A test waits for the thing it is waiting for: see
//! `support::poll`, which parks only between looks at a condition.
//!
//! The check reads test code, which is every file under `tests/` and the test
//! module that closes a source file. A wait that is an offset rather than a
//! wait is recorded in `maestro-quality.toml` as an exception, with its reason.

use crate::rust_source::{Exceptions, Finding, Sources, parse, repository, unsettled};

/// The rule's identifier in `maestro-quality.toml`.
const RULE: &str = "TST-001";

/// The modules whose `sleep` function waits on time.
const CLOCKS: &[&str] = &["thread", "time", "task"];

/// Every wait on time in the test code of `sources`.
fn waits(sources: &Sources) -> Vec<Finding> {
    let mut found = Vec::new();
    for (path, text) in sources {
        let parsed = parse(text);
        let whole_file = path.starts_with("tests/");
        for named in parsed
            .named
            .iter()
            .filter(|named| whole_file || named.in_tests)
        {
            let [.., clock, sleep] = named.segments.as_slice() else {
                continue;
            };
            if sleep != "sleep" || !CLOCKS.contains(&clock.as_str()) {
                continue;
            }
            found.push(Finding {
                rule: RULE,
                path: path.clone(),
                line: parsed.tokens.get(named.at).map_or(0, |token| token.line),
                item: parsed.function_at(named.at).unwrap_or_default().to_owned(),
                message: format!(
                    "`{clock}::sleep` waits on time; wait for the condition instead, or record in \
                     maestro-quality.toml why this one is an offset"
                ),
            });
        }
    }
    found
}

#[test]
fn no_test_waits_on_time() {
    let sources = repository();
    assert!(!sources.is_empty(), "no Rust file was found");
    let failures = unsettled(RULE, &waits(&sources), &Exceptions::repository());
    assert!(
        failures.is_empty(),
        "A test waits on time; wait for the condition instead, or record why this one is an \
         offset in maestro-quality.toml:\n  {}\n",
        failures.join("\n  ")
    );
}

/// `sources` from `(path, text)` pairs.
fn files(pairs: &[(&str, &str)]) -> Sources {
    pairs
        .iter()
        .map(|&(path, text)| (path.to_owned(), text.to_owned()))
        .collect()
}

/// The findings of `sources` as `path:line item` strings.
fn places(sources: &Sources) -> Vec<String> {
    waits(sources)
        .iter()
        .map(|found| format!("{}:{} {}", found.path, found.line, found.item))
        .collect()
}

#[test]
fn a_sleep_in_a_test_file_is_refused_with_the_function_that_waits() {
    let sources = files(&[(
        "tests/it/slow.rs",
        "fn helper() {}\n\n#[test]\nfn the_reply_arrives() {\n    std::thread::sleep(d);\n}\n",
    )]);
    assert_eq!(places(&sources), ["tests/it/slow.rs:5 the_reply_arrives"]);
}

#[test]
fn only_the_test_module_of_a_source_file_is_test_code() {
    let sources = files(&[(
        "src/pace.rs",
        "pub fn pace() {\n    std::thread::sleep(gap);\n}\n#[cfg(test)]\nmod tests {\n    \
         #[test]\n    fn it_paces() {\n        thread::sleep(gap);\n    }\n}\n",
    )]);
    assert_eq!(places(&sources), ["src/pace.rs:8 it_paces"]);
}

#[test]
fn a_sleep_imported_by_name_is_refused_where_it_is_imported() {
    let sources = files(&[
        ("tests/it/one.rs", "use std::thread::sleep;\n"),
        (
            "tests/it/two.rs",
            "use std::{thread::{self, sleep}, time};\n",
        ),
        ("tests/it/three.rs", "use tokio::time::sleep as nap;\n"),
    ]);
    assert_eq!(
        places(&sources),
        [
            "tests/it/one.rs:1 ",
            "tests/it/three.rs:1 ",
            "tests/it/two.rs:1 "
        ]
    );
}

#[test]
fn a_sleep_named_only_in_a_comment_or_a_string_is_not_a_wait() {
    let sources = files(&[(
        "tests/it/prose.rs",
        "// std::thread::sleep(d) would be flaky\nfn quiet() {\n    let text = \"thread::sleep\";\n    \
         let raw = r#\"time::sleep\"#;\n    /* task::sleep */\n}\n",
    )]);
    assert_eq!(places(&sources), Vec::<String>::new());
}

#[test]
fn another_sleep_function_and_a_park_are_not_a_wait() {
    let sources = files(&[(
        "tests/it/other.rs",
        "fn quiet() {\n    clock.sleep(d);\n    scheduler::sleep(d);\n    std::thread::park();\n}\n",
    )]);
    assert_eq!(places(&sources), Vec::<String>::new());
}
