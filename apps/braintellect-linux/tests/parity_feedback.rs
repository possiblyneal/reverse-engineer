//! Feedback-graph layer replayed through the parity harness: the baseline
//! stage inputs go in, the pass flags, counter triggers and notes come out and
//! must match what the real app produced (`docs/reverse-engineering/
//! braintellect2/audio-feedback.md`, "Run 3 decision capture" and "Run 4
//! Capture").

mod harness;

use braintellect_linux::feedback::{
    trigger_all, BandAmplitudes, Mode, NoteSelection, PeakFrequencies, Smart2Design, Threshold,
    TriggerPath, RATE_HZ,
};
use harness::assertions;
use harness::fixtures::{self, Decision, NoteBlock, TriggerBlock};

/// Where the counter's timer reads when the first block arrives. The app
/// constructs the counter before the first sample; the baseline's first fire
/// 250 ms after the first block shows the offset is at least 250 ms, and the
/// exact value is unresolved (`audio-feedback.md`), so replay starts the timer
/// one trigger epoch before the first block.
const TIMER_BEFORE_FIRST_BLOCK_MS: i64 = 500;

fn trigger(time_ms: u64) -> Decision {
    Decision {
        time_ms,
        kind: "Trigger".into(),
        old: 0,
        new: 1,
    }
}

fn replay() -> (Vec<bool>, Vec<Decision>) {
    let mut path = TriggerPath::smart2(-TIMER_BEFORE_FIRST_BLOCK_MS, Mode::Original);
    let (mut passes, mut triggers) = (Vec::new(), Vec::new());
    for block in fixtures::feedback_blocks() {
        let out = path.process_block(block.time_ms as i64, &block.values);
        passes.extend(out.passes);
        if out.trigger {
            triggers.push(trigger(block.time_ms));
        }
    }
    (passes, triggers)
}

#[test]
fn threshold_pass_flags_match_the_baseline() {
    let expected: Vec<bool> = fixtures::feedback_blocks()
        .iter()
        .flat_map(|b| b.passes)
        .collect();
    let (actual, _) = replay();
    assertions::pass_flags(&expected, &actual).expect("pass flag parity");
}

#[test]
fn counter_triggers_match_the_baseline() {
    let expected: Vec<Decision> = fixtures::feedback_decisions()
        .into_iter()
        .filter(|d| d.kind == "Trigger")
        .collect();
    assert!(expected.len() > 20, "baseline holds many triggers");
    let (_, actual) = replay();
    assertions::decisions(&expected, &actual).expect("trigger parity");
}

// Run 4: the stages upstream of the threshold, replayed from the logged
// `ExprTriggerAll` amplitudes and `ExprNoteSelection` PeakFrequencies.

fn amplitudes(block: &TriggerBlock) -> Vec<BandAmplitudes> {
    (0..4)
        .map(|n| BandAmplitudes {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: block.amplitudes[0][n],
            f: block.amplitudes[1][n],
            g: block.amplitudes[2][n],
            h: block.amplitudes[3][n],
        })
        .collect()
}

fn peaks(block: &NoteBlock) -> Vec<PeakFrequencies> {
    (0..4)
        .map(|n| PeakFrequencies(std::array::from_fn(|ch| block.peaks[ch][n])))
        .collect()
}

/// Replay the amplitude blocks through the trigger path: every pass flag, and
/// whether the counter fired in each block.
fn replay_amplitudes() -> (Vec<bool>, Vec<bool>) {
    let mut path = TriggerPath::smart2(-TIMER_BEFORE_FIRST_BLOCK_MS, Mode::Original);
    let (mut passes, mut fired) = (Vec::new(), Vec::new());
    for block in fixtures::smart2_trigger_blocks() {
        let out = path.process_amplitudes(block.time_ms as i64, block.control, &amplitudes(&block));
        passes.extend(out.passes);
        fired.push(out.trigger);
    }
    (passes, fired)
}

#[test]
fn trigger_all_amplitudes_reproduce_the_baseline_pass_flags() {
    let expected: Vec<bool> = fixtures::smart2_trigger_blocks()
        .iter()
        .flat_map(|b| b.passes)
        .collect();
    let (actual, _) = replay_amplitudes();
    assertions::pass_flags(&expected, &actual).expect("pass flag parity");
}

#[test]
fn trigger_all_amplitudes_reproduce_the_baseline_triggers() {
    let expected: Vec<Decision> = fixtures::smart2_decisions()
        .into_iter()
        .filter(|d| d.kind == "Trigger")
        .collect();
    assert!(expected.len() > 15, "baseline holds many triggers");
    let (_, fired) = replay_amplitudes();
    let actual: Vec<Decision> = fixtures::smart2_trigger_blocks()
        .iter()
        .zip(fired)
        .filter(|(_, fired)| *fired)
        .map(|(block, _)| trigger(block.time_ms))
        .collect();
    assertions::decisions(&expected, &actual).expect("trigger parity");
}

#[test]
fn note_selection_reproduces_the_baseline_note_every_block() {
    let blocks = fixtures::smart2_note_blocks();
    let mut selection = NoteSelection::smart2(Mode::Original);
    let actual: Vec<i32> = blocks
        .iter()
        .map(|b| selection.process_block(b.control, &peaks(b)))
        .collect();
    let expected: Vec<i32> = blocks.iter().map(|b| b.note).collect();
    assertions::notes(&expected, &actual).expect("note parity");
}

/// The note decisions the sound element received, replayed at the baseline's
/// fire blocks. The counter's own fires are matched separately, within the
/// harness timing window: its timer reads at sub-block resolution, so a fire
/// can land one block off, which the note would expose.
#[test]
fn notes_at_the_baseline_fires_reproduce_the_baseline_note_decisions() {
    let fire_blocks = fixtures::smart2_fire_blocks();
    let times: Vec<u64> = fixtures::smart2_trigger_blocks()
        .iter()
        .map(|b| b.time_ms)
        .collect();
    let mut selection = NoteSelection::smart2(Mode::Original);
    let mut actual = Vec::new();
    for (index, note) in fixtures::smart2_note_blocks()
        .iter()
        .enumerate()
        .take(times.len())
    {
        // Within a block the trigger path runs before the note path, so a
        // fire sees the note of the previous block.
        if fire_blocks.contains(&index) {
            actual.push(Decision {
                time_ms: times[index],
                kind: "Note".into(),
                old: 0,
                new: u32::try_from(selection.latest()).expect("MIDI notes are non-negative"),
            });
        }
        selection.process_block(note.control, &peaks(note));
    }
    let expected: Vec<Decision> = fixtures::smart2_decisions()
        .into_iter()
        .filter(|d| d.kind == "Note")
        .collect();
    assert!(expected.len() > 15, "baseline holds many notes");
    assertions::decisions(&expected, &actual).expect("note decision parity");
}

/// The design's `ExprTriggerAll` expression, one branch per Evaluator code
/// (`audio-feedback.md`, "SMART-2 Evaluator"): the Ch1/Ch2 codes read A..D, the
/// Ch3/Ch4 codes E..H, and any other code sums E..H.
#[test]
fn trigger_all_decodes_every_evaluator_code_of_the_design() {
    let amp = BandAmplitudes {
        a: 1.0,
        b: 2.0,
        c: 4.0,
        d: 8.0,
        e: 16.0,
        f: 32.0,
        g: 64.0,
        h: 128.0,
    };
    let want = [
        (11, 15.0),
        (12, 1.0),
        (13, 2.0),
        (81, 12.0),
        (82, 4.0),
        (83, 8.0),
        (111, 240.0),
        (112, 16.0),
        (113, 32.0),
        (181, 192.0),
        (182, 64.0),
        (183, 128.0),
        (999, 240.0),
    ];
    for (code, value) in want {
        assert_eq!(trigger_all(code, amp), value, "code {code}");
    }
}

#[test]
fn the_built_in_design_is_the_one_decoded_from_the_real_apps_records() {
    let decoded = fixtures::smart2_design();
    assert_eq!(Smart2Design::smart2(decoded.source.clone()), decoded);
}

/// Feed `values` through the SMART-2 threshold of `mode`, one block per value, and
/// return the pass flag of each.
fn threshold_passes(mode: Mode, values: &[f64]) -> Vec<bool> {
    let mut threshold = Threshold::smart2(RATE_HZ, mode);
    values
        .iter()
        .map(|&v| {
            let passed = threshold.push(v);
            threshold.end_block();
            passed
        })
        .collect()
}

/// A full 5 s ring of one repeated value, then one larger value pushes a copy of the
/// repeat out of the ring, then a value between the two is tested. The original erases
/// every copy (`audio-feedback.md`, "Threshold window erases by value"), so its window
/// holds only the larger value and the threshold jumps to it; the fixed window keeps the
/// other copies and the threshold stays at the repeated value.
fn repeat_then_larger_then_between() -> Vec<f64> {
    let mut values = vec![5.0; RATE_HZ * 5];
    values.extend([20.0, 10.0]);
    values
}

#[test]
fn original_mode_shrinks_the_threshold_window_when_values_repeat() {
    let passes = threshold_passes(Mode::Original, &repeat_then_larger_then_between());
    assert!(
        passes.last().copied().expect("flags"),
        "10 is below the threshold 20 of the one-entry window"
    );
}

#[test]
fn fixed_mode_keeps_the_threshold_window_at_its_ring_size_when_values_repeat() {
    let passes = threshold_passes(Mode::Fixed, &repeat_then_larger_then_between());
    assert!(
        !passes.last().copied().expect("flags"),
        "10 is not below the threshold 5 of the full window"
    );
}

/// The notes of three long blocks: code 182 pins the tracker minimum at channel 3's 0 Hz,
/// code 183 its maximum at channel 4's 40 Hz, then `code` selects from channel 3 = 10 Hz
/// and channel 4 = 30 Hz. Each block outlasts the 62-sample smoother, so the last note
/// reads the selected value alone: 10 Hz is note 49, 30 Hz note 67, their mean note 58.
fn notes_under(mode: Mode, code: u32) -> Vec<i32> {
    let block = |ch3: f32, ch4: f32| vec![PeakFrequencies([1.0, 2.0, ch3, ch4]); 100];
    let mut selection = NoteSelection::smart2(mode);
    [
        (182, block(0.0, 40.0)),
        (183, block(0.0, 40.0)),
        (code, block(10.0, 30.0)),
    ]
    .iter()
    .map(|(control, peaks)| selection.process_block(*control, peaks))
    .collect()
}

/// The Evaluator's Ch3/Ch4 Band 10 codes are 112/113, but the design's `ExprNoteSelection`
/// lists 102/103, so the original averages channels 3 and 4 for them
/// (`audio-feedback.md`, "Channels 3n4 note codes").
#[test]
fn original_mode_averages_channels_3_and_4_for_the_band_10_pair_codes() {
    for code in [112, 113] {
        assertions::notes(&[40, 76, 58], &notes_under(Mode::Original, code))
            .unwrap_or_else(|e| panic!("code {code}: {e:?}"));
    }
}

/// Fixed mode reads 112 as channel 3 and 113 as channel 4, as 182/183 are read for Band 80.
#[test]
fn fixed_mode_selects_channel_3_for_112_and_channel_4_for_113() {
    assertions::notes(&[40, 76, 49], &notes_under(Mode::Fixed, 112)).expect("112 is channel 3");
    assertions::notes(&[40, 76, 67], &notes_under(Mode::Fixed, 113)).expect("113 is channel 4");
}
