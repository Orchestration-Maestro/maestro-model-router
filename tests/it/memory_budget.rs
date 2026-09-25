//! The memory budget, read from its variable.
//!
//! Its own module rather than a case inside the eviction tests: this is the
//! rule for one variable, and the eviction tests state their budget directly
//! with `Budget::new`.
//!
//! The rule is read through `Budget::from_variable`, against a machine each
//! test states as a `Fixed` probe. Changing the process environment instead
//! is `unsafe` in Rust 2024 and races every other test in the binary, and
//! asking the real machine made the answer depend on whichever machine ran
//! the tests, and on whether its tools answered in time.

use maestro_model_router::admission::Budget;
use maestro_model_router::memory::{DeviceMemory, Fixed, Probe};

const VARIABLE: &str = "MAESTRO_MEMORY_BUDGET_MIB";

/// A machine with one 32 GiB device, which derives a budget of its own.
fn a_machine_with_a_device() -> Probe {
    Probe::Fixed(Fixed {
        device: Some(DeviceMemory {
            total_mib: 32_768,
            used_mib: 0,
        }),
        ..Fixed::default()
    })
}

/// A machine whose tools say nothing about what it holds.
fn a_machine_that_says_nothing() -> Probe {
    Probe::Fixed(Fixed::default())
}

#[test]
fn a_numeric_budget_is_used_as_given() {
    assert_eq!(
        Budget::from_variable(Some("24576".into()), a_machine_with_a_device())
            .expect("a numeric budget is accepted")
            .limit_mib(),
        Some(24576),
        "the variable is used as given, whatever the machine would have chosen"
    );
}

#[test]
fn an_unset_budget_is_the_one_the_machine_sets_for_itself() {
    let unset = Budget::from_variable(None, a_machine_with_a_device())
        .expect("an unset budget is not an error");
    let machine = Budget::derived(a_machine_with_a_device());
    assert_eq!(
        unset.limit_mib(),
        machine.limit_mib(),
        "unset means the budget the machine sets for itself: {}",
        unset.source()
    );
    assert!(
        unset.limit_mib().is_some(),
        "a machine that can say what it holds gets a ceiling"
    );
}

#[test]
fn an_empty_budget_is_read_as_unset_rather_than_as_nothing() {
    assert_eq!(
        Budget::from_variable(Some("".into()), a_machine_with_a_device())
            .expect("an empty value is unset, not an error")
            .limit_mib(),
        Budget::derived(a_machine_with_a_device()).limit_mib(),
        "`export MAESTRO_MEMORY_BUDGET_MIB=` is a slip, and reading it as a \
         budget of nothing would refuse every model on the machine; it means \
         what unset means"
    );
}

#[test]
fn a_machine_that_says_nothing_is_left_with_no_budget() {
    assert_eq!(
        Budget::from_variable(None, a_machine_that_says_nothing())
            .expect("an unset budget is not an error")
            .limit_mib(),
        None,
        "only a machine that cannot say what it holds is left with no ceiling"
    );
}

#[test]
fn a_mistyped_budget_is_refused_naming_the_variable_and_the_value() {
    let failure = Budget::from_variable(Some("plenty".into()), a_machine_with_a_device())
        .expect_err("a budget someone typed wrongly must not become no budget")
        .to_string();
    assert!(
        failure.contains(VARIABLE) && failure.contains("plenty"),
        "the refusal names the variable and what it carried: {failure}"
    );
}
