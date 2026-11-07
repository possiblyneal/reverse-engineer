//! Holds the clone's Circle-Balance chart values to the original's, over one stored exercise:
//! the raw hook log of the original's report (`0x5c4550`, `CircleGraphChart` input add) against
//! the clone's `report` over that exercise's stored LMH series
//! (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Circle-Balance design").
//!
//! `S62_CAPTURE` names the hook log (default `tmp/s62/s62-report.bin`) and `S62_LMH` the TSV
//! `scripts/lmh-extract.py` writes from the exercise's decoded type-9 XML (default
//! `tmp/s62/lmh146.tsv`); both are vendor-derived and stay under `tmp/`. The committed
//! `circle_balance_*.tsv` fixtures are the same data, asserted in `tests/parity_circle_balance.rs`.

mod harness;

use std::fs;

use harness::{assertions, fixtures, inputs};

/// The value of each `0x5c4550` call, in call order: tag 31 records are
/// `[31, len, ts, this, count, scale, values..]`.
fn chart_inputs(capture: &[u8]) -> Vec<f32> {
    let words: Vec<f64> = capture
        .chunks(8)
        .map(|b| f64::from_le_bytes(b.try_into().expect("whole f64 records")))
        .collect();
    let mut values = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let (tag, len) = (words[i] as u32, words[i + 1] as usize);
        assert!(
            len >= 2 && i + len <= words.len(),
            "record at word {i} has length {len}: a truncated or corrupt log"
        );
        if tag == 31 {
            assert_eq!(
                (words[i + 4], words[i + 5]),
                (1.0, 1.0),
                "one value at scale 1"
            );
            values.push(words[i + 6] as f32);
        }
        i += len;
    }
    values
}

#[test]
#[ignore = "needs the report's hook log and the exercise's LMH series under tmp/s62"]
fn the_clone_reproduces_the_originals_chart_inputs_exactly() {
    let capture = inputs::path("S62_CAPTURE", "tmp/s62/s62-report.bin");
    let expected = chart_inputs(&fs::read(&capture).unwrap_or_else(|e| panic!("{capture:?}: {e}")));
    let lmh = inputs::path("S62_LMH", "tmp/s62/lmh146.tsv");
    let lmh = fs::read_to_string(&lmh).unwrap_or_else(|e| panic!("{lmh:?}: {e}"));
    let (lmh, duration_s) = fixtures::parse_lmh(&lmh);
    let report = fixtures::circle_balance_report_in_call_order(&lmh, duration_s);
    assertions::chart_values(&expected, &report).unwrap_or_else(|m| panic!("{}", m.0));
}
