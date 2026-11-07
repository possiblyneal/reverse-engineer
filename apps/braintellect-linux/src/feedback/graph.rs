//! The SMART-2 feedback graph from the Source output to the trigger and note
//! decisions: `BandPassFilter` and `AggregatorFilter` elements, `PeakFrequency`,
//! then the trigger path and note selection. The caller runs the Source filter
//! chain ([`SourceFilters`](super::SourceFilters), built from
//! [`Smart2Design::source`]) and feeds its output in volts. Element
//! parameters come from the design (`BT-SMART-2-v5.1.2`), passed in as
//! [`Smart2Design`], not from per-band constants.
//!
//! The Evaluator (`coherenceChecker`) is not implemented; its control code is an
//! input of each block.

use std::fmt;

use super::band_pass::{BandPass, BandPassDesign};
use super::peak_frequency::{Peak, PeakDesign, PeakFrequency};
use super::source::SourceDesign;
use super::{
    BandAmplitudes, BlockClock, Mode, NoteSelection, PeakFrequencies, Smoother, TriggerPath,
    RATE_HZ,
};

/// Channels the graph carries (the B-2's four, in resolved order).
pub const CHANNELS: usize = 4;
/// Samples per stream block.
pub const BLOCK_LEN: usize = 4;
/// Label of the 1-3 Hz band-pass elements (`BPF Ch<n> Band 10`).
pub const BAND_LOW: &str = "Band 10";
/// Label of the 23-36 Hz band-pass elements (`BPF Ch<n> Band 80`).
pub const BAND_HIGH: &str = "Band 80";

/// One `BandPassFilter` element, with the `AggregatorFilter` it feeds.
#[derive(Debug, Clone, PartialEq)]
pub struct BandPassSpec {
    /// Zero-based channel in resolved order.
    pub channel: usize,
    pub label: String,
    pub design: BandPassDesign,
}

/// One `PeakFrequency` element.
#[derive(Debug, Clone, PartialEq)]
pub struct PeakSpec {
    pub channel: usize,
    pub design: PeakDesign,
}

/// The design properties the graph is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct Smart2Design {
    pub rate_hz: usize,
    /// The Source filter chain, which the caller runs ahead of the graph.
    pub source: SourceDesign,
    pub band_passes: Vec<BandPassSpec>,
    pub peaks: Vec<PeakSpec>,
    /// `averageEpoch` of the `continuous` Aggregators, in seconds.
    pub aggregator_epoch_s: usize,
}

/// A design the graph cannot be built from.
#[derive(Debug, PartialEq, Eq)]
pub struct DesignError(pub String);

impl fmt::Display for DesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unusable SMART-2 design: {}", self.0)
    }
}

impl std::error::Error for DesignError {}

/// The stage outputs for one sample.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleOutput {
    /// Band-pass amplitudes in volts, in the design's `band_passes` order.
    pub bands: Vec<f32>,
    /// Aggregator outputs in volts, parallel to `bands`.
    pub aggregates: Vec<f32>,
    /// PeakFrequency of channels 1..4.
    pub peaks: [Peak; CHANNELS],
}

/// What one block produced.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDecision {
    pub samples: Vec<SampleOutput>,
    pub passes: Vec<bool>,
    pub trigger: bool,
    /// The note the block's samples select.
    pub note: i32,
    /// The note the sound element reads when the counter fired in this block.
    pub note_at_trigger: Option<i32>,
}

pub struct Smart2Graph {
    mode: Mode,
    design: Smart2Design,
    bands: Vec<BandPass>,
    aggregators: Vec<Smoother>,
    peaks: Vec<PeakFrequency>,
    trigger: TriggerPath,
    notes: NoteSelection,
}

impl Smart2Graph {
    /// `timer_start_ms` is the counter timer's reading at construction (see
    /// [`TriggerPath::smart2`]). `mode` is the one explicit choice between the original's
    /// behaviour and the fixed one ([`Mode`]); in [`Mode::Fixed`] every band-pass takes
    /// the scalar-head sum order.
    ///
    /// # Errors
    /// Fails if a band-pass or PeakFrequency names a channel outside 1..4, or the
    /// design lacks a PeakFrequency for a channel or the Band 10/80 elements
    /// `ExprTriggerAll` reads, its sample rate is not the 500 Hz the trigger and
    /// note windows are sized for, or its Aggregator `averageEpoch` is zero.
    pub fn new(design: Smart2Design, timer_start_ms: i64, mode: Mode) -> Result<Self, DesignError> {
        let err = |msg: String| Err(DesignError(msg));
        if design.rate_hz != RATE_HZ {
            return err(format!(
                "sample rate {} Hz, not {RATE_HZ} Hz",
                design.rate_hz
            ));
        }
        if design.aggregator_epoch_s == 0 {
            return err("zero Aggregator averageEpoch".into());
        }
        if design
            .band_passes
            .iter()
            .map(|b| b.channel)
            .chain(design.peaks.iter().map(|p| p.channel))
            .any(|c| c >= CHANNELS)
        {
            return err("channel outside the 4 resolved channels".into());
        }
        for channel in 0..CHANNELS {
            if !design.peaks.iter().any(|p| p.channel == channel) {
                return err(format!("no PeakFrequency for channel {}", channel + 1));
            }
            for label in [BAND_LOW, BAND_HIGH] {
                if band_index(&design, channel, label).is_none() {
                    return err(format!("no {label} band-pass for channel {}", channel + 1));
                }
            }
        }
        let rate = design.rate_hz;
        let window = rate * design.aggregator_epoch_s;
        Ok(Self {
            mode,
            bands: design
                .band_passes
                .iter()
                .map(|b| {
                    let mut band = BandPass::new(&b.design, rate, BLOCK_LEN);
                    band.set_scalar_head(mode == Mode::Fixed);
                    band
                })
                .collect(),
            aggregators: design
                .band_passes
                .iter()
                .map(|_| Smoother::new(window))
                .collect(),
            peaks: design
                .peaks
                .iter()
                .map(|p| PeakFrequency::new(&p.design, rate))
                .collect(),
            trigger: TriggerPath::smart2(timer_start_ms, mode),
            notes: NoteSelection::smart2(),
            design,
        })
    }

    /// Which band-pass output blocks start on the IPP kernel's scalar head (see
    /// [`BandPass::set_scalar_head`]), in the design's `band_passes` order, from the
    /// next block on. The app's choice follows the output buffer's heap address, so a
    /// replay has to supply it; the default is all `false`. [`Mode::Fixed`] ignores the
    /// call: its order is the fixed all-scalar-head policy (`audio-feedback.md`, "Live
    /// sum order").
    ///
    /// # Panics
    /// Panics if `heads` is not one flag per band-pass.
    pub fn set_scalar_heads(&mut self, heads: &[bool]) {
        assert_eq!(heads.len(), self.bands.len(), "one flag per band-pass");
        if self.mode == Mode::Fixed {
            return;
        }
        for (band, &head) in self.bands.iter_mut().zip(heads) {
            band.set_scalar_head(head);
        }
    }

    /// One sample, in volts per channel, that has already left the Source filters:
    /// the band-pass, Aggregator and PeakFrequency elements only.
    pub fn push_filtered(&mut self, volts: [f64; CHANNELS]) -> SampleOutput {
        let bands: Vec<f32> = self
            .design
            .band_passes
            .iter()
            .zip(&mut self.bands)
            .map(|(spec, band)| band.push(volts[spec.channel]) as f32)
            .collect();
        let aggregates = bands
            .iter()
            .zip(&mut self.aggregators)
            .map(|(&v, agg)| agg.push(v))
            .collect();
        let mut peaks = [Peak::default(); CHANNELS];
        for (spec, peak) in self.design.peaks.iter().zip(&mut self.peaks) {
            peaks[spec.channel] = peak.push(volts[spec.channel]);
        }
        SampleOutput {
            bands,
            aggregates,
            peaks,
        }
    }

    /// One block of samples in volts that have already left the Source filters
    /// (see [`Self::push_filtered`]), under the Evaluator code `control`, read at
    /// `clock`. Within a block the trigger path runs before the note path.
    pub fn process_filtered_block(
        &mut self,
        clock: impl Into<BlockClock>,
        control: u32,
        block: &[[f64; CHANNELS]],
    ) -> BlockDecision {
        let samples = block.iter().map(|&s| self.push_filtered(s)).collect();
        self.decide(clock.into(), control, samples)
    }

    fn decide(
        &mut self,
        clock: BlockClock,
        control: u32,
        samples: Vec<SampleOutput>,
    ) -> BlockDecision {
        let amplitudes: Vec<BandAmplitudes> = samples
            .iter()
            .map(|s| {
                let at =
                    |channel, label| s.bands[band_index(&self.design, channel, label).unwrap()];
                BandAmplitudes {
                    a: at(0, BAND_LOW),
                    b: at(1, BAND_LOW),
                    c: at(0, BAND_HIGH),
                    d: at(1, BAND_HIGH),
                    e: at(2, BAND_LOW),
                    f: at(3, BAND_LOW),
                    g: at(2, BAND_HIGH),
                    h: at(3, BAND_HIGH),
                }
            })
            .collect();
        let out = self.trigger.process_amplitudes(clock, control, &amplitudes);
        let note_at_trigger = out.trigger.then(|| self.notes.latest());
        let peaks: Vec<PeakFrequencies> = samples
            .iter()
            .map(|s| PeakFrequencies(s.peaks.map(|p| p.frequency_hz as f32)))
            .collect();
        let note = self.notes.process_block(control, &peaks);
        BlockDecision {
            samples,
            passes: out.passes,
            trigger: out.trigger,
            note,
            note_at_trigger,
        }
    }
}

fn band_index(design: &Smart2Design, channel: usize, label: &str) -> Option<usize> {
    design
        .band_passes
        .iter()
        .position(|b| b.channel == channel && b.label == label)
}

impl Smart2Design {
    /// The `BT-SMART-2-v5.1.2` design with the clinician's Source settings:
    /// eight 128-tap Hamming band-passes (Band 10 at 1-3 Hz, Band 80 at 23-36 Hz,
    /// per channel), a Hann `PeakFrequency` per channel (6-14 Hz on channels 1
    /// and 2, 7-15 Hz on 3 and 4) and the `continuous` Aggregator with a 60 s
    /// epoch, at 500 Hz. `tests/parity_feedback.rs` checks it against the design
    /// fixture decoded from the real app's records.
    #[must_use]
    pub fn smart2(source: SourceDesign) -> Self {
        let band = |label: &str, low_hz, high_hz, channel| BandPassSpec {
            channel,
            label: label.to_owned(),
            design: BandPassDesign {
                low_hz,
                high_hz,
                order: 128,
            },
        };
        let peak = |channel, low_hz, high_hz| PeakSpec {
            channel,
            design: PeakDesign {
                fft_order: 9,
                low_hz,
                high_hz,
            },
        };
        Self {
            rate_hz: RATE_HZ,
            source,
            band_passes: [(BAND_LOW, 1.0, 3.0), (BAND_HIGH, 23.0, 36.0)]
                .into_iter()
                .flat_map(|(label, low, high)| {
                    (0..CHANNELS).map(move |channel| band(label, low, high, channel))
                })
                .collect(),
            peaks: vec![
                peak(0, 6.0, 14.0),
                peak(1, 6.0, 14.0),
                peak(2, 7.0, 15.0),
                peak(3, 7.0, 15.0),
            ],
            aggregator_epoch_s: 60,
        }
    }
}
