//! The Circle-Balance report design (`B-2-Circle-Balance-fromBAGSD-v5.1.1`) against the
//! original's design XML. Its chart values have no captured baseline yet; the ignored
//! `tests/replay_s62.rs` reports them.

mod harness;

use braintellect_linux::feedback::circle_balance::CircleBalanceDesign;

use harness::{assertions, fixtures};

#[test]
fn the_built_in_circle_balance_design_is_the_one_decoded_from_the_original() {
    assertions::circle_balance_design(
        &fixtures::circle_balance_design(),
        &CircleBalanceDesign::circle_balance(),
    )
    .unwrap_or_else(|m| panic!("{}", m.0));
}
