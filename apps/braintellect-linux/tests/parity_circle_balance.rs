//! The Circle-Balance report design (`B-2-Circle-Balance-fromBAGSD-v5.1.1`) against the
//! original's design XML, and its chart values against the ones the original's chart inputs
//! received when its report ran over the same stored exercise (`circle_balance_chart.tsv`).
//! The ignored `tests/replay_s62.rs` holds the clone to the raw capture.

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

#[test]
fn the_balance_meter_values_match_the_original_chart_inputs_exactly() {
    let (lmh, duration_s) = fixtures::circle_balance_lmh();
    let report = fixtures::circle_balance_report_in_call_order(&lmh, duration_s);
    assertions::chart_values(&fixtures::circle_balance_chart(), &report)
        .unwrap_or_else(|m| panic!("{}", m.0));
}
