//! Loads the scrubbed baseline fixtures. Regenerate them with
//! `apps/braintellect-linux/scripts/make-fixtures.py`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

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

/// Baseline microvolts per frame: counter and the 8 payload samples.
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
    read("feedback_decisions.tsv")
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
/// payload microvolt values in payload order (channel order is unresolved).
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
