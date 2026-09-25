//! The guest rule, driven from values as the rest of admission's policy is.
//!
//! A guest is a model loaded into free room. An idle one gives its room back
//! before any other model, the coldest guest first, and only then the coldest
//! of the rest; one being read from is no more a candidate than any other
//! model. These cases drive `Budget::admit_with_guests` with the fixtures the
//! cases of `decision` drive `Budget::admit` with. They have a file of their
//! own because `decision.rs` has no room left for them under the 500 lines of
//! code the source rules allow a file.

use crate::catalog::Residency;

use super::budget::Budget;
use super::decision::Decision;
use super::decision::tests::{loaded, on_demand, wanted};

#[test]
fn an_idle_guest_goes_before_any_other_model_the_coldest_guest_first() {
    let budget = Budget::new(Some(4_096));
    // 3000 held and 1500 wanted, so 404 has to go, and either guest frees
    // that much alone. "chat" answered longest ago, which is what chose
    // the model to unload before guests existed. The colder guest is
    // listed last, so a policy that took guests in the order it was
    // handed them would take the other one.
    let held = [
        on_demand("warm-guest", 500, 1),
        on_demand("chat", 2_000, 30),
        on_demand("cold-guest", 500, 10),
    ];
    let guests = ["warm-guest".to_owned(), "cold-guest".to_owned()];

    assert_eq!(
        budget.admit_with_guests(&held, &guests, &wanted("other-chat", 1_500), None),
        Decision::Unload(vec!["cold-guest".to_owned()]),
        "a model loaded into free room gives its room back before any \
         other, and the coldest of them goes first"
    );
}

#[test]
fn a_busy_guest_is_never_unloaded_so_the_idle_model_goes_instead() {
    // The known limit: a guest goes first only while nothing is reading
    // from it. This one is colder than the idle model and would free
    // enough on its own, and a search is still reading its answer -- so
    // the idle model goes, a chat model included, as before guests.
    let budget = Budget::new(Some(10_000));
    let held = [
        loaded("searching", 6_000, Residency::OnDemand, true, 100),
        on_demand("idle", 2_000, 1),
    ];

    assert_eq!(
        budget.admit_with_guests(
            &held,
            &["searching".to_owned()],
            &wanted("wanted", 3_000),
            None
        ),
        Decision::Unload(vec!["idle".to_owned()]),
        "a model being read from is never a candidate, guest or not"
    );
}
