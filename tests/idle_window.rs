//! The idle window, read from its variable.
//!
//! Its own target, for the same reason `memory_budget.rs` has one: it is the
//! rule for one variable, and a reader looking for it should find it by name.
//!
//! The rule is read through `IdleWindow::from_variable`, which takes the
//! variable's value as an argument. Changing the process environment instead
//! is `unsafe` in Rust 2024 and races every other test in the binary.

#![cfg(test)]

use maestro_model_router::idle::IdleWindow;

const VARIABLE: &str = "MAESTRO_IDLE_UNLOAD_SECONDS";

#[test]
fn an_unset_window_means_nothing_is_unloaded_for_sitting_idle() {
    assert_eq!(
        IdleWindow::from_variable(None)
            .expect("an unset variable is not an error")
            .seconds(),
        None,
        "unset means no window, which means nothing is ever unloaded for \
         sitting idle"
    );
}

#[test]
fn an_empty_window_is_off_rather_than_zero_seconds() {
    assert_eq!(
        IdleWindow::from_variable(Some("".into()))
            .expect("an empty value is off, not an error")
            .seconds(),
        None,
        "`export MAESTRO_IDLE_UNLOAD_SECONDS=` is a slip, and reading it as a \
         window of zero seconds would unload every on-demand model on every \
         sweep"
    );
}

#[test]
fn a_numeric_window_is_used_as_given() {
    assert_eq!(
        IdleWindow::from_variable(Some("3600".into()))
            .expect("a numeric window is accepted")
            .seconds(),
        Some(3600),
        "the variable is used as given"
    );
}

#[test]
fn a_mistyped_window_is_refused_naming_the_variable_and_the_value() {
    let Err(failure) = IdleWindow::from_variable(Some("soon".into())) else {
        panic!("a window someone typed wrongly must not become no window");
    };
    let failure = failure.to_string();
    assert!(
        failure.contains(VARIABLE) && failure.contains("soon"),
        "the refusal names the variable and what it carried: {failure}"
    );
}
