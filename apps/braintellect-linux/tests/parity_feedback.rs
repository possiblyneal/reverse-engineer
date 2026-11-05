//! Feedback-graph layer replayed through the parity harness: the baseline
//! threshold-stage input goes in, the pass flags and counter triggers come out
//! and must match what the real app produced (`docs/reverse-engineering/
//! braintellect2/audio-feedback.md`, "Run 3 decision capture").

mod harness;

use braintellect_linux::feedback::TriggerPath;
use harness::assertions;
use harness::fixtures::{self, Decision};

/// Where the counter's timer reads when the first block arrives. The app
/// constructs the counter before the first sample; the baseline's first fire
/// 250 ms after the first block shows the offset is at least 250 ms, and the
/// exact value is unresolved (`audio-feedback.md`), so replay starts the timer
/// one trigger epoch before the first block.
const TIMER_BEFORE_FIRST_BLOCK_MS: i64 = 500;

fn replay() -> (Vec<bool>, Vec<Decision>) {
    let mut path = TriggerPath::smart2(-TIMER_BEFORE_FIRST_BLOCK_MS);
    let (mut passes, mut triggers) = (Vec::new(), Vec::new());
    for block in fixtures::feedback_blocks() {
        let out = path.process_block(block.time_ms as i64, &block.values);
        passes.extend(out.passes);
        if out.trigger {
            triggers.push(Decision {
                time_ms: block.time_ms,
                kind: "Trigger".into(),
                old: 0,
                new: 1,
            });
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
