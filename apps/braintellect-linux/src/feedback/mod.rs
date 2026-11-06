//! The SMART-2 feedback graph, as configured in `BT-SMART-2-v5.1.2`: the trigger
//! path (`ExprTriggerAll` into `Threshold 20` into `ThresholdCounter`) and note
//! selection (`ExprNoteSelection`).
//!
//! Evidence and open items: `docs/reverse-engineering/braintellect2/audio-feedback.md`.
//! [`SourceFilters`] is the `Source` software filter chain the caller runs on the
//! decoded EEG; [`Smart2Graph`] runs the rest from its output: band-pass
//! `OutAmplitude`, Aggregator and `PeakFrequency`, then the two paths, which also
//! take those stages' logged values directly.
//! Not implemented: the Evaluator (its control code is an input), `OutAmplitude`
//! of the sound element and the `SoundElement` note playback.

mod band_pass;
mod counter;
mod fir;
mod graph;
mod iir;
mod note;
mod peak_frequency;
mod smoother;
mod source;
mod threshold;
mod trigger_all;

pub use band_pass::{BandPass, BandPassDesign};
pub use counter::{BlockClock, Counter};
pub use graph::{
    BandPassSpec, BlockDecision, DesignError, PeakSpec, SampleOutput, Smart2Design, Smart2Graph,
    BAND_HIGH, BAND_LOW, BLOCK_LEN, CHANNELS,
};
pub use note::{NoteSelection, PeakFrequencies};
pub use peak_frequency::{Peak, PeakDesign, PeakFrequency};
pub use source::{Interference1, Interference2, SourceDesign, SourceFilters};
pub use threshold::Threshold;
pub use trigger_all::{trigger_all, BandAmplitudes};

pub use smoother::Smoother;

/// Sample rate of the stream the graph runs at; the trigger and note windows
/// are sized for it.
pub const RATE_HZ: usize = 500;
/// `averageEpoch` of `Threshold 20`, in milliseconds.
const AVERAGE_EPOCH_MS: usize = 125;
/// `ExprTriggerAll` yields volts; the threshold stage works in microvolts.
const VOLTS_TO_MICROVOLTS: f64 = 1e6;

/// What one block of threshold input produced.
pub struct BlockOutput {
    /// One pass flag per input value.
    pub passes: Vec<bool>,
    /// The counter fired in this block.
    pub trigger: bool,
}

/// `ExprTriggerAll`, then `Threshold 20` and `ThresholdCounter`, fed one block
/// at a time.
pub struct TriggerPath {
    smoother: Smoother,
    threshold: Threshold,
    counter: Counter,
}

impl TriggerPath {
    /// The SMART-2 configuration at the 500 Hz sample rate. `timer_start_ms` is
    /// the counter timer's reading at construction, on the same clock as the
    /// clock passed to [`TriggerPath::process_block`].
    pub fn smart2(timer_start_ms: i64) -> Self {
        Self {
            smoother: Smoother::new(RATE_HZ * AVERAGE_EPOCH_MS / 1000),
            threshold: Threshold::smart2(RATE_HZ),
            counter: Counter::smart2(timer_start_ms),
        }
    }

    /// Process one block of band amplitudes under the Evaluator code `control`,
    /// read at `clock`: `ExprTriggerAll`, microvolt scaling, the `averageEpoch`
    /// smoothing, then [`TriggerPath::process_block`].
    pub fn process_amplitudes(
        &mut self,
        clock: impl Into<BlockClock>,
        control: u32,
        block: &[BandAmplitudes],
    ) -> BlockOutput {
        let mut smoothed = Vec::with_capacity(block.len());
        for &amplitudes in block {
            let microvolts =
                (f64::from(trigger_all(control, amplitudes)) * VOLTS_TO_MICROVOLTS) as f32;
            smoothed.push(f64::from(self.smoother.push(microvolts)));
        }
        self.process_block(clock, &smoothed)
    }

    /// Process one block of smoothed inputs read at `clock`.
    pub fn process_block(&mut self, clock: impl Into<BlockClock>, values: &[f64]) -> BlockOutput {
        let passes: Vec<bool> = values.iter().map(|&v| self.threshold.push(v)).collect();
        self.threshold.end_block();
        let trigger = self.counter.process_block(clock.into(), &passes);
        BlockOutput { passes, trigger }
    }
}
