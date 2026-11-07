//! The SMART-2 feedback graph from the Source output to the trigger and note
//! decisions: `BandPassFilter` and `AggregatorFilter` elements, `PeakFrequency`,
//! then the trigger path and note selection. The caller runs the Source filter
//! chain ([`SourceFilters`](super::SourceFilters), built from
//! [`Smart2Design::source`]) and feeds its output in volts. Element
//! parameters come from the design (`BT-SMART-2-v5.1.2`), passed in as
//! [`Smart2Design`], not from per-band constants.
//!
//! The Evaluator ([`Evaluator`]) turns the Aggregators' averages, of the band-pass
//! amplitudes and of the Ch3/Ch4 `Coherence` elements, into the control code. A block runs
//! under the Evaluator's code ([`Smart2Graph::process_evaluated_block`]) or, for a parity
//! test, under a logged one ([`Smart2Graph::process_filtered_block`]).

use std::fmt;

use super::band_pass::{BandPass, BandPassDesign};
use super::coherence::Coherence;
use super::evaluator::{balances, BALANCES, COHERENCES};
use super::peak_frequency::{Peak, PeakDesign, PeakFrequency};
use super::source::SourceDesign;
use super::{
    BandAmplitudes, BlockClock, Evaluator, Mode, NoteSelection, PeakFrequencies, Smoother,
    TriggerPath, RATE_HZ,
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

/// One `Coherence` element (`processingStyle` `Relaxed`), with the `AggregatorFilter` it
/// feeds. Its band-pass filters both inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct CoherenceSpec {
    /// Zero-based channels of `InSignal1` and `InSignal2`, in resolved order.
    pub channels: [usize; 2],
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
    pub coherences: Vec<CoherenceSpec>,
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
    /// The Evaluator code the block ran under.
    pub control: u32,
    /// The Balance values after the block's last sample (`evaluator::BALANCES` series:
    /// Ch1/Ch2 Band 10, Ch1/Ch2 Band 80, Ch3/Ch4 Band 10, Ch3/Ch4 Band 80).
    pub balances: [f32; BALANCES],
    /// The Coherence Aggregators' averages after the block's last sample
    /// (`evaluator::COHERENCES` series: Ch3/Ch4 Band 10, Ch3/Ch4 Band 80).
    pub coherences: [f32; COHERENCES],
}

/// A `Coherence` element and the Aggregator it feeds.
struct CoherencePath {
    channels: [usize; 2],
    element: Coherence,
    aggregator: Smoother,
}

pub struct Smart2Graph {
    mode: Mode,
    design: Smart2Design,
    bands: Vec<BandPass>,
    aggregators: Vec<Smoother>,
    /// The Evaluator's Coherence series in order.
    coherences: [CoherencePath; COHERENCES],
    peaks: Vec<PeakFrequency>,
    trigger: TriggerPath,
    notes: NoteSelection,
    evaluator: Evaluator,
}

impl Smart2Graph {
    /// `timer_start_ms` is the counter timer's reading at construction (see
    /// [`TriggerPath::smart2`]). `mode` is the one explicit choice between the original's
    /// behaviour and the fixed one ([`Mode`]); in [`Mode::Fixed`] every band-pass takes
    /// the scalar-head sum order.
    ///
    /// # Errors
    /// Fails if a band-pass, Coherence or PeakFrequency names a channel outside 1..4, or
    /// the design lacks a PeakFrequency for a channel, the Band 10/80 elements
    /// `ExprTriggerAll` reads or the Band 10/80 Coherence the Evaluator reads, its sample
    /// rate is not the 500 Hz the trigger and note windows are sized for, or its Aggregator `averageEpoch` is zero.
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
            .chain(design.coherences.iter().flat_map(|c| c.channels))
            .any(|c| c >= CHANNELS)
        {
            return err("channel outside the 4 resolved channels".into());
        }
        let rate = design.rate_hz;
        let window = rate * design.aggregator_epoch_s;
        let coherences = [BAND_LOW, BAND_HIGH].map(|label| {
            design
                .coherences
                .iter()
                .find(|c| c.label == label)
                .map(|c| CoherencePath {
                    channels: c.channels,
                    element: Coherence::new(&c.design, rate, BLOCK_LEN),
                    aggregator: Smoother::new(window),
                })
        });
        let [Some(low), Some(high)] = coherences else {
            return err("no Band 10 and Band 80 Coherence".into());
        };
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
        Ok(Self {
            coherences: [low, high],
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
            notes: NoteSelection::smart2(mode),
            evaluator: Evaluator::default(),
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
    /// (see [`Self::push_filtered`]), under the logged Evaluator code `control`, read at
    /// `clock`. Within a block the trigger path runs before the note path. The
    /// Evaluator still runs, so the block's Balance and Coherence values are reported.
    pub fn process_filtered_block(
        &mut self,
        clock: impl Into<BlockClock>,
        control: u32,
        block: &[[f64; CHANNELS]],
    ) -> BlockDecision {
        let samples: Vec<SampleOutput> = block.iter().map(|&s| self.push_filtered(s)).collect();
        let inputs = self.evaluate(block, &samples);
        self.decide(clock.into(), control, inputs, samples)
    }

    /// Like [`Self::process_filtered_block`], under the code the Evaluator decides from
    /// the block's samples: a code decided at the end of an epoch applies to the block that
    /// completes it.
    pub fn process_evaluated_block(
        &mut self,
        clock: impl Into<BlockClock>,
        block: &[[f64; CHANNELS]],
    ) -> BlockDecision {
        let samples: Vec<SampleOutput> = block.iter().map(|&s| self.push_filtered(s)).collect();
        let inputs = self.evaluate(block, &samples);
        let control = self.evaluator.code();
        self.decide(clock.into(), control, inputs, samples)
    }

    /// The Evaluator code now in effect.
    #[must_use]
    pub fn control(&self) -> u32 {
        self.evaluator.code()
    }

    /// Run the Coherence elements and their Aggregators over `block`, then the Evaluator on
    /// the latest Balance and Coherence values, which are returned.
    fn evaluate(
        &mut self,
        block: &[[f64; CHANNELS]],
        samples: &[SampleOutput],
    ) -> ([f32; BALANCES], [f32; COHERENCES]) {
        let last = samples.last().expect("a block holds samples");
        let latest = balances(self.amplitudes(&last.aggregates));
        let coherences = self.coherences.each_mut().map(|path| {
            let [a, b] = path.channels;
            let inputs: Vec<[f64; 2]> = block.iter().map(|s| [s[a], s[b]]).collect();
            let value = path.element.process_block(&inputs);
            // The element outputs its value for each of the block's samples.
            let mut average = value;
            for _ in block {
                average = path.aggregator.push(value);
            }
            average
        });
        self.evaluator.end_block(samples.len(), latest, coherences);
        (latest, coherences)
    }

    /// Bands A..H out of per-band values in the design's `band_passes` order.
    fn amplitudes(&self, values: &[f32]) -> BandAmplitudes {
        let at = |channel, label| values[band_index(&self.design, channel, label).unwrap()];
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
    }

    fn decide(
        &mut self,
        clock: BlockClock,
        control: u32,
        (balances, coherences): ([f32; BALANCES], [f32; COHERENCES]),
        samples: Vec<SampleOutput>,
    ) -> BlockDecision {
        let amplitudes: Vec<BandAmplitudes> =
            samples.iter().map(|s| self.amplitudes(&s.bands)).collect();
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
            control,
            balances,
            coherences,
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
    /// per channel), a `Relaxed` Ch3/Ch4 `Coherence` on each of the two bands with the
    /// same filters, a Hann `PeakFrequency` per channel (6-14 Hz on channels 1
    /// and 2, 7-15 Hz on 3 and 4) and the `continuous` Aggregator with a 60 s
    /// epoch, at 500 Hz. `tests/parity_feedback.rs` checks it against the design
    /// fixture decoded from the real app's records.
    #[must_use]
    pub fn smart2(source: SourceDesign) -> Self {
        let design = |low_hz, high_hz| BandPassDesign {
            low_hz,
            high_hz,
            order: 128,
        };
        let band = |label: &str, low_hz, high_hz, channel| BandPassSpec {
            channel,
            label: label.to_owned(),
            design: design(low_hz, high_hz),
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
            coherences: [(BAND_LOW, 1.0, 3.0), (BAND_HIGH, 23.0, 36.0)]
                .into_iter()
                .map(|(label, low, high)| CoherenceSpec {
                    channels: [2, 3],
                    label: label.to_owned(),
                    design: design(low, high),
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
