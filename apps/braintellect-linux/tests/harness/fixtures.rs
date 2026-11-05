//! Loads the scrubbed baseline fixtures. Regenerate them with
//! `apps/braintellect-linux/scripts/make-fixtures.py`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use braintellect_linux::feedback::{
    BandPassDesign, BandPassSpec, Interference2, PeakDesign, PeakSpec, Smart2Design, SourceDesign,
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
    read("wire.txt")
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

/// One block of the run-4 `ExprTriggerAll` stage: the counter's clock reading,
/// the Evaluator code, the Band 10/80 amplitudes E..H in volts (`[input][sample]`)
/// and the pass flags the app's threshold produced from them.
pub struct TriggerBlock {
    pub time_ms: u64,
    pub control: u32,
    pub amplitudes: [[f32; 4]; 4],
    pub passes: [bool; 4],
}

/// The run-4 `ExprTriggerAll` blocks, in order.
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

/// One block of the run-4 `ExprNoteSelection` stage: the Evaluator code, the
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

/// The run-4 `ExprNoteSelection` blocks, in order.
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

/// The run-4 `Trigger` (counter fire) and `Note` (the note the sound element
/// received at a fire) decisions over the span of [`smart2_trigger_blocks`],
/// as [`feedback_decisions`] encodes them.
pub fn smart2_decisions() -> Vec<Decision> {
    time_kind_value_decisions("smart2_decisions.tsv")
}

/// The blocks of the run-4 counter fires (the `Trigger` rows' value column).
pub fn smart2_fire_blocks() -> Vec<usize> {
    read("smart2_decisions.tsv")
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|c| c[1] == "Trigger")
        .map(|c| c[2].parse().expect("block"))
        .collect()
}

/// The SMART-2 design the run-4 stages were logged under, read from the design
/// XML by `make-fixtures.py`.
pub fn smart2_design() -> Smart2Design {
    let mut design = Smart2Design {
        rate_hz: 0,
        source: SourceDesign {
            interference2: Interference2::Off,
            low_pass_hz: 0.0,
            high_pass_hz: 0.0,
        },
        band_passes: vec![],
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

/// Decoded microvolts per sample from the first logged sample on, with the
/// sample's index relative to it.
pub fn smart2_eeg() -> Vec<(i64, [f64; 4])> {
    read("smart2_eeg.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 5, "index and 4 channels");
            (
                c[0].parse().expect("index"),
                std::array::from_fn(|i| c[1 + i].parse().expect("microvolts")),
            )
        })
        .collect()
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
pub fn smart2_bandpass_stages() -> Vec<[f32; 7]> {
    read("smart2_bandpass_stages.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 7, "input, two bands of two stages, peak pair");
            std::array::from_fn(|i| {
                f32::from_bits(u32::from_str_radix(c[i], 16).expect("float32 bits"))
            })
        })
        .collect()
}

/// One live Source-chain sample of channel 0, in volts: the band-stop input, then the
/// band-stop, low-pass and high-pass outputs, as the app held them in float32.
pub fn smart2_source_stages() -> Vec<[f32; 4]> {
    read("smart2_source_stages.tsv")
        .lines()
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 4, "band-stop input and three stage outputs");
            std::array::from_fn(|i| {
                f32::from_bits(u32::from_str_radix(c[i], 16).expect("float32 bits"))
            })
        })
        .collect()
}
