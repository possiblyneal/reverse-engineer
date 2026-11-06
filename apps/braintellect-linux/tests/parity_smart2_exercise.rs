//! Parity of the whole SMART-2 exercise (run 8): the decoded EEG the app consumed,
//! replayed through the Source chain, band-passes, PeakFrequency, Aggregators,
//! thresholds, counter and note selection, against the Trigger and Note decisions
//! the app made over about four minutes.
//!
//! The Source chain starts warm: it runs the earlier stream with the interference
//! filter off, and the 45 Hz low-pass is switched in cold at the graph's second
//! sample, which reproduces the app's logged Source output bit for bit.
//!
//! Inputs the clone does not derive are the logged ones: the Evaluator code, the
//! counter timer's clock reads and the 16-tap-mean sum order of each band-pass call.

mod harness;

use braintellect_linux::feedback::BLOCK_LEN;

use harness::assertions;
use harness::fixtures::{self, Decision};
use harness::run8::replay;

fn expected(kind: &str) -> Vec<Decision> {
    fixtures::run8_decisions()
        .into_iter()
        .filter(|d| d.kind == kind)
        .collect()
}

#[test]
fn trigger_decisions_match_the_baseline_over_the_whole_exercise() {
    let run = replay();
    let actual: Vec<Decision> = run
        .decisions
        .iter()
        .zip(&run.blocks)
        .filter(|(decision, _)| decision.trigger)
        .map(|(_, block)| Decision {
            time_ms: u64::try_from(block.read_ms).expect("time"),
            kind: "Trigger".into(),
            old: 0,
            new: 1,
        })
        .collect();
    let expected = expected("Trigger");
    assert_eq!(expected.len(), 415);
    assertions::decisions(&expected, &actual).expect("trigger parity from the decoded EEG");
}

/// Every Note decision exact, as the harness compares them.
#[test]
fn note_decisions_match_the_baseline_exactly() {
    let run = replay();
    let actual: Vec<Decision> = run
        .decisions
        .iter()
        .zip(&run.blocks)
        .filter_map(|(decision, block)| {
            decision.note_at_trigger.map(|note| Decision {
                time_ms: u64::try_from(block.read_ms).expect("time"),
                kind: "Note".into(),
                old: 0,
                new: u32::try_from(note).expect("MIDI notes are non-negative"),
            })
        })
        .collect();
    assertions::decisions(&expected("Note"), &actual).expect("note parity");
}

#[test]
fn band_amplitudes_match_the_logged_values_bit_for_bit_from_the_first_block() {
    let run = replay();
    let logged = fixtures::run8_amplitudes();
    assert_eq!(logged.len(), fixtures::RUN8_AMPLITUDE_BLOCKS);
    for (b, row) in logged.iter().enumerate() {
        for n in 0..BLOCK_LEN {
            for band in 0..4 {
                let at = run.band_index[4 + band];
                assert_eq!(
                    run.decisions[b].samples[n].bands[at].to_bits(),
                    row[band * BLOCK_LEN + n].to_bits(),
                    "block {b} sample {n} band {}",
                    ["E", "F", "G", "H"][band]
                );
            }
        }
    }
}
