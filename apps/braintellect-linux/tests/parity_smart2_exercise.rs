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
use harness::run8::{
    deviation, replay, replay_fixed, replay_switching_at, replay_with_flags, Deviation,
};

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

/// The live sum-order choice (`audio-feedback.md`, "Live sum order"): the original's order
/// depends on heap alignment per process, so a live run fixes one order for every band-pass
/// call. Measured against the logged order of run 8 (120462 of 240008 band-pass calls take
/// the scalar head), all-scalar-head departs less than all-SIMD, and neither changes a
/// Trigger or a Note decision.
#[test]
fn the_live_sum_order_choice_moves_amplitudes_but_no_decision() {
    let logged = replay();
    let simd = deviation(logged, &replay_with_flags(false));
    let scalar = deviation(logged, &replay_with_flags(true));
    assert_eq!(
        simd,
        Deviation {
            amplitudes: 126_070,
            amplitudes_total: 960_032,
            blocks: 28_702,
            blocks_total: 30_001,
            triggers: 0,
            notes: 0,
        }
    );
    assert_eq!(
        scalar,
        Deviation {
            amplitudes: 125_013,
            amplitudes_total: 960_032,
            blocks: 28_655,
            blocks_total: 30_001,
            triggers: 0,
            notes: 0,
        }
    );
    assert!(scalar.amplitudes < simd.amplitudes);
}

/// Fixed mode switches the 45 Hz low-pass in at the first graph sample, where the app's
/// switch lands on a thread-timed sample (start + 1 in run 8). Observable as the band
/// amplitudes: with the sum order held equal (fixed mode's all-scalar-head), the fixed
/// replay is the original replay switched at the start and not the one switched at
/// start + 1.
#[test]
fn fixed_mode_switches_the_low_pass_at_the_first_graph_block() {
    let fixed = replay_fixed();
    let at_start = replay_switching_at(0, true);
    let one_late = replay_switching_at(1, true);
    assert_eq!(deviation(fixed, &at_start).amplitudes, 0);
    assert!(deviation(fixed, &one_late).amplitudes > 0);
}

/// Run 8's decisions under fixed mode against original mode (`audio-feedback.md`, "Fixed
/// mode against original mode"): 24 of the 415 fires move to another block and 24 of the 415
/// notes at a fire differ, a count of blocks whose decision differs.
#[test]
fn fixed_mode_changes_24_trigger_and_24_note_decisions_of_run_8() {
    let d = deviation(replay(), replay_fixed());
    assert_eq!((d.triggers, d.notes), (24, 24));
}
