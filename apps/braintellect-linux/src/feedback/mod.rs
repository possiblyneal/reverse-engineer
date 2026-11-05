//! The SMART-2 feedback graph's trigger path: `Threshold 20` feeding
//! `ThresholdCounter`, as configured in `BT-SMART-2-v5.1.2`.
//!
//! Evidence and open items: `docs/reverse-engineering/braintellect2/audio-feedback.md`.
//! Not implemented (unresolved or not captured): the band-pass, Aggregator and
//! `ExprTriggerAll` stages upstream of the threshold, the threshold's own
//! `averageEpoch` smoothing, per-channel `PeakFrequency`, `ExprNoteSelection`
//! channel order, and the `SoundElement` note mapping.

mod counter;
mod threshold;

pub use counter::Counter;
pub use threshold::Threshold;

/// What one block of threshold input produced.
pub struct BlockOutput {
    /// One pass flag per input value.
    pub passes: Vec<bool>,
    /// The counter fired in this block.
    pub trigger: bool,
}

/// `Threshold 20` then `ThresholdCounter`, fed one block at a time.
pub struct TriggerPath {
    threshold: Threshold,
    counter: Counter,
}

impl TriggerPath {
    /// The SMART-2 configuration at the 500 Hz sample rate. `timer_start_ms` is
    /// the counter timer's reading at construction, on the same clock as the
    /// `now_ms` passed to [`TriggerPath::process_block`].
    pub fn smart2(timer_start_ms: i64) -> Self {
        Self {
            threshold: Threshold::smart2(500),
            counter: Counter::smart2(timer_start_ms),
        }
    }

    /// Process one block of smoothed inputs read at clock time `now_ms`.
    pub fn process_block(&mut self, now_ms: i64, values: &[f64]) -> BlockOutput {
        let passes: Vec<bool> = values.iter().map(|&v| self.threshold.push(v)).collect();
        self.threshold.end_block();
        let trigger = self.counter.process_block(now_ms, &passes);
        BlockOutput { passes, trigger }
    }
}
