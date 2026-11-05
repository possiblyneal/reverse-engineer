//! Feedback-graph layer replayed through the parity harness: the baseline
//! stage inputs go in, the pass flags, counter triggers and notes come out and
//! must match what the real app produced (`docs/reverse-engineering/
//! braintellect2/audio-feedback.md`, "Run 3 decision capture" and "Run 4
//! Capture").

mod harness;

use braintellect_linux::feedback::{BandAmplitudes, NoteSelection, PeakFrequencies, TriggerPath};
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
    let mut path = TriggerPath::smart2(-TIMER_BEFORE_FIRST_BLOCK_MS);
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
    let mut path = TriggerPath::smart2(-TIMER_BEFORE_FIRST_BLOCK_MS);
    let (mut passes, mut fired) = (Vec::new(), Vec::new());
    for block in fixtures::smart2_trigger_blocks() {
        let out = path
            .process_amplitudes(block.time_ms as i64, block.control, &amplitudes(&block))
            .expect("observed Evaluator codes");
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
    let mut selection = NoteSelection::smart2();
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
    let mut selection = NoteSelection::smart2();
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
