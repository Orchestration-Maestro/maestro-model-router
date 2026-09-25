//! Where catalog locations resolve against.
//!
//! Its own module rather than a case inside the supervision tests: this stays
//! red until the launch command exists, while supervision turns green before
//! it, and one module that is half green tells a reader nothing.
//!
//! The rule is read through `models_root_from`, which takes the variable's
//! value and the home directory as arguments. Changing the process
//! environment instead is `unsafe` in Rust 2024 and races every other test in
//! the binary, so no case here touches it.

use std::path::PathBuf;

use maestro_model_router::launch::models_root_from;

const VARIABLE: &str = "MAESTRO_MODELS_ROOT";

/// A home directory that names no machine.
fn home() -> PathBuf {
    PathBuf::from("/somewhere/home")
}

#[test]
fn a_configured_root_is_used_as_given() {
    assert_eq!(
        models_root_from(Some("/somewhere/models".into()), Some(home()))
            .expect("a configured root is used as given"),
        PathBuf::from("/somewhere/models"),
        "the variable wins when it is set"
    );
}

#[test]
fn an_unset_root_falls_back_to_models_under_the_home_directory() {
    assert_eq!(
        models_root_from(None, Some(home())).expect("the fallback applies"),
        PathBuf::from("/somewhere/home/models"),
        "otherwise 'models' under the home directory, which is where the \
         current router already looks"
    );
}

#[test]
fn an_empty_root_is_read_as_unset_rather_than_as_nothing() {
    assert_eq!(
        models_root_from(Some("".into()), Some(home())).expect("the fallback applies"),
        PathBuf::from("/somewhere/home/models"),
        "`export MAESTRO_MODELS_ROOT=` is a slip, and resolving every model \
         against nothing would blame the catalog for a missing file"
    );
}

#[test]
fn no_root_and_no_home_directory_is_refused_naming_the_variable() {
    let failure = models_root_from(None, None)
        .expect_err("there is nowhere to resolve against")
        .to_string();
    assert!(
        failure.contains(VARIABLE),
        "the refusal names what to set: {failure}"
    );
}
