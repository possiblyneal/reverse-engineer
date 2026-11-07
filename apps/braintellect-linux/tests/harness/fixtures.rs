//! Loads the scrubbed baseline fixtures. Regenerate them with
//! `apps/braintellect-linux/scripts/make-fixtures.py`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use braintellect_linux::feedback::circle_balance::{
    Anchor, Axis, CircleBalanceDesign, Graph, LmhBand, Meter, MeterInput, TimeWindow,
};
use braintellect_linux::feedback::{
    BandPassDesign, BandPassSpec, CoherenceSpec, Interference1, Interference2, PeakDesign,
    PeakSpec, Smart2Design, SourceDesign,
};

fn read(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("fixture hex"))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Tx,
    Rx,
}

/// One write to, or reply from, the controller, as the app saw it.
#[derive(Clone, Debug)]
pub struct WireEvent {
    pub dir: Dir,
    pub bytes: Vec<u8>,
}

/// The baseline handshake and start sequence, in order (replies carry the
/// scrubbed serial placeholder).
pub fn wire() -> Vec<WireEvent> {
    wire_file("wire.txt")
}

/// A live handshake from the real B-2 (`handshake_live_a.txt`: the first
/// connection after switch-on; `handshake_live_b.txt`: a reconnect), laid out like
/// [`wire`] with the commands the clone writes and the serial scrubbed.
pub fn live_handshake(tag: char) -> Vec<WireEvent> {
    wire_file(&format!("handshake_live_{tag}.txt"))
}

fn wire_file(name: &str) -> Vec<WireEvent> {
    read(name)
        .lines()
        .map(|l| {
            let (dir, hex) = l.split_once(' ').expect("DIR hex");
            WireEvent {
                dir: if dir == "TX" { Dir::Tx } else { Dir::Rx },
                bytes: unhex(hex),
            }
        })
        .collect()
}

/// The commands the real app wrote, in order.
pub fn commands_sent() -> Vec<Vec<u8>> {
    wire()
        .into_iter()
        .filter(|e| e.dir == Dir::Tx)
        .map(|e| e.bytes)
        .collect()
}

/// A note/trigger decision from the baseline control log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub time_ms: u64,
    pub kind: String,
    pub old: u32,
    pub new: u32,
}

pub fn decisions() -> Vec<Decision> {
    read("decisions.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            Decision {
                time_ms: c[0].parse().expect("time"),
                kind: c[1].to_owned(),
                old: c[2].parse().expect("old"),
                new: c[3].parse().expect("new"),
            }
        })
        .collect()
}

/// One stream frame: 16-bit big-endian counter and the 24-byte sample payload.
pub struct Frame {
    pub counter: u16,
    pub payload: [u8; 24],
}

const MARKER: [u8; 4] = [0xAC, 0xBC, 0xCC, 0x03];
const FRAME_LEN: usize = 32;

/// The baseline stream window, split into frames on the marker (the capture's
/// chunking does not align with frames).
pub fn stream_frames() -> Vec<Frame> {
    let bytes: Vec<u8> = read("stream.txt").lines().flat_map(unhex).collect();
    bytes
        .chunks(FRAME_LEN)
        .map(|f| {
            assert_eq!(f.len(), FRAME_LEN, "stream fixture ends mid-frame");
            assert_eq!(f[..4], MARKER, "stream fixture desynchronised");
            Frame {
                counter: u16::from_be_bytes([f[4], f[5]]),
                payload: f[8..].try_into().expect("24-byte payload"),
            }
        })
        .collect()
}

/// The baseline stream window as the capture delivered it: one chunk per
/// fixture line, not aligned with frames.
pub fn stream_chunks() -> Vec<Vec<u8>> {
    read("stream.txt").lines().map(unhex).collect()
}

/// Baseline microvolts per frame: counter and the 8 samples in channel order
/// (channel 1's two samples, then channel 2's, ...).
pub fn expected_microvolts() -> Vec<(u16, Vec<f64>)> {
    read("microvolts.tsv")
        .lines()
        .map(|l| {
            let mut cols = l.split('\t');
            let counter = cols.next().expect("counter").parse().expect("counter");
            (
                counter,
                cols.map(|v| v.parse().expect("microvolts")).collect(),
            )
        })
        .collect()
}

/// The frames the original consumed from the replayed no-wearer stream
/// (`consumed_stream.txt`, one 32-byte frame per line), as the transport's chunks.
pub fn consumed_stream_chunks() -> Vec<Vec<u8>> {
    read("consumed_stream.txt").lines().map(unhex).collect()
}

/// Parse volts rows (`consumed_volts.tsv`: per consumed sample, the original's logged volts of
/// channels 0 to 3 as float64 bit patterns) into microvolts per channel, `[channel][sample]`.
/// The `* 1e6` is the clone's own last step (`stream.rs`); the logged volts are the independent
/// value.
pub fn consumed_microvolts_from(text: &str) -> Vec<Vec<f64>> {
    let mut channels = vec![Vec::new(); 4];
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 4, "four channels per consumed sample");
        for (c, col) in cols.iter().enumerate() {
            let bits = u64::from_str_radix(col, 16).expect("float64 bits");
            channels[c].push(f64::from_bits(bits) * 1e6);
        }
    }
    channels
}

/// The original's logged consumed microvolts for the committed window, `[channel][sample]`.
pub fn consumed_microvolts() -> Vec<Vec<f64>> {
    consumed_microvolts_from(&read("consumed_volts.tsv"))
}

/// Baseline stored-record fields (`key -> value`).
#[derive(Clone)]
pub struct StoredRecord(pub BTreeMap<String, String>);

impl StoredRecord {
    pub fn number(&self, key: &str) -> f64 {
        self.0[key]
            .parse()
            .unwrap_or_else(|_| panic!("{key} is not numeric"))
    }
}

pub fn stored_record() -> StoredRecord {
    StoredRecord(
        read("record.tsv")
            .lines()
            .map(|l| {
                let (k, v) = l.split_once('\t').expect("key<TAB>value");
                (k.to_owned(), v.to_owned())
            })
            .collect(),
    )
}

/// One block of the threshold stage as the real app processed it: the counter's
/// clock reading, the 4 smoothed threshold inputs, and the pass flags it produced.
pub struct FeedbackBlock {
    pub time_ms: u64,
    pub values: [f64; 4],
    pub passes: [bool; 4],
}

/// The baseline threshold-stage blocks, in order.
pub fn feedback_blocks() -> Vec<FeedbackBlock> {
    read("feedback_blocks.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 9, "time, 4 values, 4 pass flags");
            FeedbackBlock {
                time_ms: c[0].parse().expect("time"),
                values: std::array::from_fn(|i| c[1 + i].parse().expect("value")),
                passes: std::array::from_fn(|i| c[5 + i] == "1"),
            }
        })
        .collect()
}

/// The baseline feedback decisions, aligned to [`feedback_blocks`]: `Trigger`
/// (a counter fire, `new` = 1) and `Note` (the note input the sound element
/// received, `new` = its value). `old` is 0.
pub fn feedback_decisions() -> Vec<Decision> {
    time_kind_value_decisions("feedback_decisions.tsv")
}

fn time_kind_value_decisions(name: &str) -> Vec<Decision> {
    read(name)
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            Decision {
                time_ms: c[0].parse().expect("time"),
                kind: c[1].to_owned(),
                old: 0,
                new: if c[1] == "Trigger" {
                    1
                } else {
                    c[2].parse().expect("note")
                },
            }
        })
        .collect()
}

/// One received EEG frame of the window: arrival time, counter, and the 8
/// microvolt values in channel order.
pub struct EegFrame {
    pub time_ms: u64,
    pub counter: u16,
    pub microvolts: [f64; 8],
}

/// The baseline EEG window, aligned to [`feedback_blocks`] (same zero).
pub fn feedback_eeg() -> Vec<EegFrame> {
    read("feedback_eeg.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            EegFrame {
                time_ms: c[0].parse().expect("time"),
                counter: c[1].parse().expect("counter"),
                microvolts: std::array::from_fn(|i| c[2 + i].parse().expect("microvolts")),
            }
        })
        .collect()
}

/// One block of the baseline `ExprTriggerAll` stage: the counter's clock reading,
/// the Evaluator code, the Band 10/80 amplitudes E..H in volts (`[input][sample]`)
/// and the pass flags the app's threshold produced from them.
pub struct TriggerBlock {
    pub time_ms: u64,
    pub control: u32,
    pub amplitudes: [[f32; 4]; 4],
    pub passes: [bool; 4],
}

/// The baseline `ExprTriggerAll` blocks, in order.
pub fn smart2_trigger_blocks() -> Vec<TriggerBlock> {
    read("smart2_trigger_blocks.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 22, "time, code, 16 amplitudes, 4 pass flags");
            TriggerBlock {
                time_ms: c[0].parse().expect("time"),
                control: c[1].parse().expect("code"),
                amplitudes: std::array::from_fn(|i| {
                    std::array::from_fn(|n| c[2 + 4 * i + n].parse().expect("amplitude"))
                }),
                passes: std::array::from_fn(|i| c[18 + i] == "1"),
            }
        })
        .collect()
}

/// One block of the baseline `ExprNoteSelection` stage: the Evaluator code, the
/// PeakFrequency of channels 1..4 in Hz (`[channel][sample]`) and the note the
/// app computed from them.
pub struct NoteBlock {
    pub control: u32,
    pub peaks: [[f32; 4]; 4],
    pub note: i32,
}

/// PeakFrequency resolution: sample rate / FFT order (500 Hz / 512). The fixture
/// stores each peak as its FFT bin, one hex digit per sample.
const PEAK_BIN_HZ: f32 = 500.0 / 512.0;

/// The baseline `ExprNoteSelection` blocks, in order.
pub fn smart2_note_blocks() -> Vec<NoteBlock> {
    read("smart2_note_blocks.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 6, "code, 4 channels of bins, note");
            NoteBlock {
                control: c[0].parse().expect("code"),
                peaks: std::array::from_fn(|ch| {
                    let bins = c[1 + ch].as_bytes();
                    assert_eq!(bins.len(), 4, "one bin per sample");
                    std::array::from_fn(|n| {
                        let bin = (bins[n] as char).to_digit(16).expect("hex bin");
                        bin as f32 * PEAK_BIN_HZ
                    })
                }),
                note: c[5].parse().expect("note"),
            }
        })
        .collect()
}

/// The baseline `Trigger` (counter fire) and `Note` (the note the sound element
/// received at a fire) decisions over the span of [`smart2_trigger_blocks`],
/// as [`feedback_decisions`] encodes them.
pub fn smart2_decisions() -> Vec<Decision> {
    time_kind_value_decisions("smart2_decisions.tsv")
}

/// The blocks of the baseline counter fires (the `Trigger` rows' value column).
pub fn smart2_fire_blocks() -> Vec<usize> {
    read("smart2_decisions.tsv")
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|c| c[1] == "Trigger")
        .map(|c| c[2].parse().expect("block"))
        .collect()
}

/// The SMART-2 design the baseline stages were logged under, read from the design
/// XML by `make-fixtures.py`.
pub fn smart2_design() -> Smart2Design {
    let mut design = Smart2Design {
        rate_hz: 0,
        source: SourceDesign {
            interference1: Interference1::Off,
            interference2: Interference2::Off,
            low_pass_hz: 0.0,
            high_pass_hz: 0.0,
        },
        band_passes: vec![],
        coherences: vec![],
        peaks: vec![],
        aggregator_epoch_s: 0,
    };
    for line in read("smart2_design.tsv").lines() {
        let c: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| -> f64 { c[i].parse().expect("design number") };
        match c[0] {
            "rate" => design.rate_hz = c[1].parse().expect("rate"),
            "source" => match c[1] {
                "interferenceFilter2" => {
                    design.source.interference2 = match c[2] {
                        "25 Hz" => Interference2::Hz25,
                        "Off" => Interference2::Off,
                        other => panic!("interference filter {other}"),
                    }
                }
                "lowPassCutoff" => design.source.low_pass_hz = num(2),
                "highPassCutoff" => design.source.high_pass_hz = num(2),
                other => panic!("source property {other}"),
            },
            "bandpass" => {
                assert_eq!(c[5], "Hamming");
                design.band_passes.push(BandPassSpec {
                    channel: c[1].parse::<usize>().expect("channel") - 1,
                    label: c[2].to_owned(),
                    design: BandPassDesign {
                        low_hz: num(3),
                        high_hz: num(4),
                        order: c[6].parse().expect("order"),
                    },
                });
            }
            "coherence" => {
                assert_eq!((c[6], c[8]), ("Hamming", "Relaxed"));
                let channel = |i: usize| c[i].parse::<usize>().expect("channel") - 1;
                design.coherences.push(CoherenceSpec {
                    channels: [channel(1), channel(2)],
                    label: c[3].to_owned(),
                    design: BandPassDesign {
                        low_hz: num(4),
                        high_hz: num(5),
                        order: c[7].parse().expect("order"),
                    },
                });
            }
            "peak" => {
                assert_eq!(c[5], "Hann");
                design.peaks.push(PeakSpec {
                    channel: c[1].parse::<usize>().expect("channel") - 1,
                    design: PeakDesign {
                        fft_order: c[2].parse().expect("fft order"),
                        low_hz: num(3),
                        high_hz: num(4),
                    },
                });
            }
            "aggregator" => {
                assert_eq!(c[1], "continuous");
                design.aggregator_epoch_s = c[2].parse().expect("epoch");
            }
            other => panic!("design row {other}"),
        }
    }
    design
}

/// One block of the live Source output: each channel's 4 samples in volts as the app held
/// them (float32), and, per band-pass in the design's order, whether the block's 16-tap
/// mean ran the IPP kernel's scalar head.
pub struct StageInput {
    pub volts: [[f32; 4]; 4],
    pub scalar_heads: [bool; 8],
}

/// The live Source output of the baseline's first blocks, in order.
pub fn smart2_stage_inputs() -> Vec<StageInput> {
    read("smart2_stage_inputs.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 17, "16 samples and the head flags");
            let flags = c[16].as_bytes();
            assert_eq!(flags.len(), 8, "one flag per band-pass");
            StageInput {
                volts: std::array::from_fn(|ch| std::array::from_fn(|n| bits(c[4 * ch + n]))),
                scalar_heads: std::array::from_fn(|i| flags[i] == b'1'),
            }
        })
        .collect()
}

fn bits(hex: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(hex, 16).expect("float32 bits"))
}

/// One logged 4-sample block of the stage values.
pub struct StageBlock {
    pub control: u32,
    /// Band-pass amplitudes in volts, `[stage A..H][sample]`.
    pub bands: [[f32; 4]; 8],
    /// Aggregator outputs in volts, same layout.
    pub aggregates: [[f32; 4]; 8],
    /// PeakFrequency bin per channel and sample.
    pub peak_bins: [[usize; 4]; 4],
}

/// Stage letters A..H as `(label, zero-based channel)`.
pub const STAGES: [(&str, usize); 8] = [
    ("Band 10", 0),
    ("Band 10", 1),
    ("Band 80", 0),
    ("Band 80", 1),
    ("Band 10", 2),
    ("Band 10", 3),
    ("Band 80", 2),
    ("Band 80", 3),
];

pub fn smart2_stages() -> Vec<StageBlock> {
    read("smart2_stages.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 69, "code, 32 bands, 32 aggregates, 4 bin strings");
            let grid = |at: usize| -> [[f32; 4]; 8] {
                std::array::from_fn(|s| {
                    std::array::from_fn(|n| c[at + 4 * s + n].parse().expect("volts"))
                })
            };
            StageBlock {
                control: c[0].parse().expect("code"),
                bands: grid(1),
                aggregates: grid(33),
                peak_bins: std::array::from_fn(|ch| {
                    let bins = c[65 + ch].as_bytes();
                    std::array::from_fn(|n| {
                        (bins[n] as char).to_digit(16).expect("hex bin") as usize
                    })
                }),
            }
        })
        .collect()
}

/// One live sample of a single channel's `BandPassFilter` and `PeakFrequency` stages, in
/// float32: the shared input in volts, the Band 10 FIR output and amplitude, the Band 80
/// FIR output and amplitude, then the `PeakFrequency` frequency and amplitude.
pub struct BandStage {
    pub values: [f32; 7],
    /// Whether the sample's block ran the 16-tap mean's scalar head, for Band 10 and Band 80.
    pub scalar_heads: [bool; 2],
}

pub fn smart2_bandpass_stages() -> Vec<BandStage> {
    read("smart2_bandpass_stages.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(
                c.len(),
                9,
                "input, two bands of two stages, peak pair, two flags"
            );
            BandStage {
                values: std::array::from_fn(|i| bits(c[i])),
                scalar_heads: [c[7] == "1", c[8] == "1"],
            }
        })
        .collect()
}

/// One live Source-chain sample of channel 0, in volts: the chain input, then the
/// 45 Hz interference low-pass, band-stop, low-pass and high-pass outputs, as the app
/// held them in float32.
pub fn smart2_source_stages() -> Vec<[f32; 5]> {
    read("smart2_source_stages.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 5, "chain input and four stage outputs");
            std::array::from_fn(|i| {
                f32::from_bits(u32::from_str_radix(c[i], 16).expect("float32 bits"))
            })
        })
        .collect()
}

/// One block of the run-8 whole-exercise baseline.
#[derive(Clone, Debug)]
pub struct Run8Block {
    /// The 16-tap-mean head flags of bands A..H (`true` = scalar head).
    pub scalar_heads: [bool; 8],
    /// The Evaluator code the block ran under.
    pub control: u32,
    /// The counter timer's evaluation read, ms from block 0.
    pub read_ms: i64,
    /// The lap read taken when the counter fired, ms from block 0.
    pub lap_ms: Option<i64>,
}

/// The consumed EEG of run 8 from 4000 samples before the graph's first sample
/// (the pre-roll that warms the Source chain): raw 24-bit counts,
/// eight per frame (slots 0..4 then 4..8 are two consecutive samples of the
/// four channels).
pub fn run8_eeg() -> Vec<[i32; 8]> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    (0..)
        .map(|part| format!("run8_eeg_{part}.txt"))
        .take_while(|name| dir.join(name).exists())
        .flat_map(|name| {
            read(&name)
                .lines()
                .map(|line| {
                    std::array::from_fn(|k| {
                        let v = i32::from_str_radix(&line[6 * k..6 * k + 6], 16).expect("hex");
                        (v << 8) >> 8
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The per-block table of run 8, in block order.
pub fn run8_blocks() -> Vec<Run8Block> {
    ["run8_blocks_0.tsv", "run8_blocks_1.tsv"]
        .iter()
        .flat_map(|name| read(name).lines().map(String::from).collect::<Vec<_>>())
        .map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            let lap: i64 = c[3].parse().expect("lap");
            Run8Block {
                scalar_heads: std::array::from_fn(|k| c[0].as_bytes()[k] == b'1'),
                control: c[1].parse().expect("code"),
                read_ms: c[2].parse().expect("read"),
                lap_ms: (lap >= 0).then_some(lap),
            }
        })
        .collect()
}

/// The Trigger (value = block) and Note decisions the app made over run 8.
pub fn run8_decisions() -> Vec<Decision> {
    time_kind_value_decisions("run8_decisions.tsv")
}

/// `(block, note)` of every trigger in a `time kind value` decisions log whose Trigger value
/// is the block and whose Note follows its Trigger (`scripts/replay-extract.py`).
pub fn trigger_notes(log: &str) -> Vec<(usize, u32)> {
    let rows: Vec<Vec<&str>> = log.lines().map(|l| l.split('\t').collect()).collect();
    rows.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            assert_eq!((pair[0][1], pair[1][1]), ("Trigger", "Note"), "{pair:?}");
            (
                pair[0][2].parse().expect("block"),
                pair[1][2].parse().expect("note"),
            )
        })
        .collect()
}

/// `(block, note)` of every trigger over run 8.
pub fn run8_trigger_notes() -> Vec<(usize, u32)> {
    trigger_notes(&read("run8_decisions.tsv"))
}

/// Blocks of logged E..H amplitudes the run 8 fixtures keep (`RUN8_AMP_BLOCKS` in
/// `scripts/make-fixtures.py`).
pub const RUN8_AMPLITUDE_BLOCKS: usize = 6600;

/// Logged amplitudes of bands E..H, four samples each (E0..E3, F0.., G0.., H0..),
/// for the first [`RUN8_AMPLITUDE_BLOCKS`] blocks of the run.
pub fn run8_amplitudes() -> Vec<[f32; 16]> {
    ["run8_amplitudes_0.tsv", "run8_amplitudes_1.tsv"]
        .iter()
        .flat_map(|name| read(name).lines().map(String::from).collect::<Vec<_>>())
        .map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            std::array::from_fn(|k| bits(words[k]))
        })
        .collect()
}

/// What the original's Head2D held for one channel after a block: the line-noise
/// level (`+0x5c`) and the 12, 40 and 70 Hz band levels, in dB.
#[derive(Clone, Debug)]
pub struct SensorLevels {
    pub line_noise: f32,
    pub bands: [f32; 3],
}

/// One block of the sensors-off replay into the original (`sensor_*.tsv`): each
/// channel's 512 float32 FFT input, the 256-bin spectrum the original's `DSPFFT::process`
/// made of it, the levels the original derived from that, and the evaluator's fault flag
/// after the block. Blocks are numbered from the start
/// of the capture; only some are kept.
#[derive(Clone, Debug)]
pub struct SensorBlock {
    pub index: usize,
    pub samples: [Vec<f32>; 4],
    pub spectra: [Vec<f32>; 4],
    pub levels: [SensorLevels; 4],
    pub faulted: bool,
}

pub fn sensor_blocks() -> Vec<SensorBlock> {
    let inputs = read("sensor_inputs.tsv");
    let levels = read("sensor_levels.tsv");
    let spectra = read("sensor_spectra.tsv");
    let rows = |text: &str| -> Vec<Vec<String>> {
        text.lines()
            .map(|l| l.split('\t').map(String::from).collect())
            .collect()
    };
    let (inputs, levels, spectra) = (rows(&inputs), rows(&levels), rows(&spectra));
    assert_eq!(inputs.len(), levels.len(), "one row per block and channel");
    assert_eq!(inputs.len(), spectra.len(), "one row per block and channel");
    inputs
        .chunks(4)
        .zip(levels.chunks(4))
        .zip(spectra.chunks(4))
        .map(|((inp, lev), spec)| {
            let num = |s: &str| s.parse::<f32>().expect("float");
            SensorBlock {
                index: inp[0][0].parse().expect("block"),
                samples: std::array::from_fn(|c| inp[c][2].split(' ').map(num).collect()),
                spectra: std::array::from_fn(|c| spec[c][2].split(' ').map(num).collect()),
                levels: std::array::from_fn(|c| SensorLevels {
                    line_noise: num(&lev[c][2]),
                    bands: [num(&lev[c][3]), num(&lev[c][4]), num(&lev[c][5])],
                }),
                faulted: lev[0][6] == "1",
            }
        })
        .collect()
}

/// The Circle-Balance design's chart inputs, read from the design XML by `make-fixtures.py`
/// (`circle_balance_design.tsv`, one row per input).
pub fn circle_balance_design() -> CircleBalanceDesign {
    let inputs = read("circle_balance_design.tsv")
        .lines()
        .map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            let num = |i: usize| -> u32 { c[i].parse().expect("design number") };
            MeterInput {
                meter: match c[0] {
                    "Emotional State" => Meter::Emotional,
                    "Activation State" => Meter::Activation,
                    other => panic!("chart {other}"),
                },
                graph: match c[1] {
                    "Before" => Graph::Before,
                    "After" => Graph::After,
                    other => panic!("graph {other}"),
                },
                axis: match c[2] {
                    "top" => Axis::Top,
                    "bottom" => Axis::Bottom,
                    other => panic!("axis {other}"),
                },
                band: match c[3] {
                    "High" => LmhBand::High,
                    "Low" => LmhBand::Low,
                    other => panic!("band {other}"),
                },
                window: TimeWindow {
                    anchor: match c[4] {
                        "Start" => Anchor::Start,
                        "End" => Anchor::End,
                        other => panic!("anchor {other}"),
                    },
                    begin_s: num(5),
                    end_s: num(6),
                },
                channels: (
                    c[7].parse().expect("channel A"),
                    c[8].parse().expect("channel B"),
                ),
                expression: c[9].to_owned(),
            }
        })
        .collect();
    CircleBalanceDesign { inputs }
}
