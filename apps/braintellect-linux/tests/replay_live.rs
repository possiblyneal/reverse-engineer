//! The #33 live session's raw capture replayed through the live engine (`live::run`) over an
//! in-memory transport, against the original's run on the same capture (tickets #60, #66).
//!
//! The capture is the RX bytes the clone read from the B-2 in the #33 session, handshake
//! replies included, so reading it back in order replays that session; writes are dropped.
//! The original consumed the same capture served by `braintellect-linux emulate`; its hook log
//! is reduced by `scripts/replay-extract.py` to `blocks.tsv` (the code of every block) and
//! `decisions.tsv` (its Trigger and Note decisions).
//!
//! The engine runs in original mode from the stream's first consumed frame, with the
//! original's graph start, interference switch and clock reads, lap reads included; the
//! control code of every block and the block and note of every trigger must match exactly.
//!
//! Ignored: the capture and the extract are wearer EEG and live outside Git. Run with
//! `cargo test --release --test replay_live -- --ignored --nocapture`; `S33_CAPTURE` and
//! `S33_REPLAY_DIR` override the defaults (`tmp/s33/b2.rx` and `tmp/s33/replay` at the
//! repository root).

mod harness;

use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use braintellect_linux::acquisition::session::Session;
use braintellect_linux::capture::Recording;
use braintellect_linux::feedback::{Mode, DEFAULT_CODE};
use braintellect_linux::live::{self, Environment, GraphStart, Phase, SessionProperties, Snapshot};
use braintellect_linux::storage::Store;
use harness::audio::{small_bank, RecordingSink};
use harness::{assertions, fixtures, inputs};

/// The capture read back in order; whatever the session writes is dropped.
struct Replayed(Cursor<Vec<u8>>);

impl Read for Replayed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Replayed {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The 22 handshake replies (two query passes of 11) before the first stream frame.
const HANDSHAKE_LEN: usize = 22 * 56;
const FRAME_LEN: usize = 32;
/// The original's graph-start sample on the #33 extract, located by `replay_s48` from its
/// logged amplitudes (consumed sample 15616, before the first skipped frame).
const GRAPH_START: usize = 15616;
/// The original's start on the #33 extract: its Source filters ran every consumed sample
/// before the graph start, and its 45 Hz low-pass switched in 4 graph samples later
/// (`replay_s48`).
const ORIGINAL_START: GraphStart = GraphStart {
    mode: Mode::Original,
    warmup_samples: GRAPH_START,
    interference_switch: 4,
};

/// The capture's stream frames the original consumed, in order: each consumed sample pair of
/// `eeg` (one line per sample, four 24-bit hex counts) is the next frame carrying it, so the
/// frames the original's host skipped are left out.
fn consumed_frames<'a>(stream: &'a [u8], eeg: &str) -> Vec<&'a [u8]> {
    let samples: Vec<[i32; 4]> = eeg
        .lines()
        .map(|line| {
            std::array::from_fn(|k| {
                let v = i32::from_str_radix(&line[6 * k..6 * k + 6], 16).expect("hex");
                (v << 8) >> 8
            })
        })
        .collect();
    let counts = |frame: &[u8]| -> [i32; 8] {
        std::array::from_fn(|k| {
            let at = 8 + 3 * k;
            i32::from_be_bytes([frame[at], frame[at + 1], frame[at + 2], 0]) >> 8
        })
    };
    let mut frames = stream
        .as_chunks::<FRAME_LEN>()
        .0
        .iter()
        .map(<[u8; FRAME_LEN]>::as_slice);
    samples
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let want = [pair[0], pair[1]].concat();
            frames
                .find(|f| counts(f)[..] == want[..])
                .expect("every consumed pair is in the capture")
        })
        .collect()
}

/// `(first block, code)` of each run of equal codes.
fn code_runs(codes: &[u32]) -> Vec<(usize, u32)> {
    codes
        .iter()
        .enumerate()
        .filter(|&(b, &c)| b == 0 || codes[b - 1] != c)
        .map(|(b, &c)| (b, c))
        .collect()
}

#[test]
#[ignore = "needs the #33 capture and the original's extract, which stay outside Git"]
fn the_live_engine_on_the_33_capture_against_the_original() {
    let raw = fs::read(inputs::path("S33_CAPTURE", "tmp/s33/b2.rx")).expect("capture");
    let read = |name: &str| inputs::read("S33_REPLAY_DIR", "tmp/s33/replay", name);

    // Each block's code, its clock read, and the lap read the counter took when it fired.
    let blocks: Vec<(u32, i64, Option<i64>)> = read("blocks.tsv")
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let lap: i64 = f[3].parse().expect("lap");
            (
                f[1].parse().expect("code"),
                f[2].parse().expect("read"),
                (lap >= 0).then_some(lap),
            )
        })
        .collect();
    let stream = consumed_frames(&raw[HANDSHAKE_LEN..], &read("eeg.txt"));
    let mut bytes = raw[..HANDSHAKE_LEN].to_vec();
    bytes.extend(stream.concat());
    let session = Session::open(
        Recording::new(Replayed(Cursor::new(bytes)), io::sink()),
        live::stream_config().expect("config"),
    )
    .expect("handshake");
    let bank = small_bank();
    let db = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("live-33.db");
    let _ = fs::remove_file(&db);
    let store = Store::open(&db).expect("store");
    let mut sink = RecordingSink::default();
    let stop = AtomicBool::new(false);
    // The original's reads in its order: one as the graph is built, then one per block and
    // the lap read of each block where it fired.
    let mut reads = std::iter::once(blocks[0].1).chain(
        blocks
            .iter()
            .flat_map(|&(_, read, lap)| std::iter::once(read).chain(lap)),
    );
    let mut clock = move || {
        reads
            .next()
            .expect("the clone fired where the original did not")
    };
    let mut reports: Vec<Snapshot> = Vec::new();
    let last = live::run(
        session,
        &SessionProperties {
            duration_s: 3600,
            ..SessionProperties::default()
        },
        Environment {
            bank: &bank,
            sink: &mut sink,
            store: &store,
            stop: &stop,
            clock: &mut clock,
            graph: ORIGINAL_START,
            report: &mut |s: &Snapshot| reports.push(s.clone()),
            capture: None,
        },
    );
    assert!(
        matches!(last.phase, Phase::Saved { .. }),
        "{:?}",
        last.phase
    );

    // A code change or a trigger is reported in the frame that closes its block.
    let warmup_frames = (GRAPH_START / 2) as u64;
    let block_of =
        |s: &Snapshot| usize::try_from((s.frames - warmup_frames) / 2).expect("blocks") - 1;
    let mut clone_codes = vec![(0, DEFAULT_CODE)];
    let mut clone_notes = Vec::new();
    let mut triggers = 0;
    for s in &reports {
        if s.control != clone_codes.last().expect("default").1 {
            clone_codes.push((block_of(s), s.control));
        }
        if s.triggers != triggers {
            triggers = s.triggers;
            let note = u32::try_from(s.last_note.expect("note")).expect("MIDI");
            clone_notes.push((block_of(s), note));
        }
    }
    assert_eq!(
        (last.frames - warmup_frames) / 2,
        blocks.len() as u64,
        "every logged block ran"
    );
    let logged: Vec<u32> = blocks.iter().map(|b| b.0).collect();
    // The code of every block, from the reported changes.
    let clone: Vec<u32> = (0..logged.len())
        .map(|b| {
            clone_codes
                .iter()
                .rfind(|&&(from, _)| from <= b)
                .expect("default")
                .1
        })
        .collect();
    println!("original codes {:?}", code_runs(&logged));
    println!("clone codes {clone_codes:?}");
    assertions::control_codes(&logged, &clone).expect("per-block code parity");

    let logged_notes = fixtures::trigger_notes(&read("decisions.tsv"));
    println!(
        "triggers: original {}, clone {}; clone only {:?}; original only {:?}",
        logged_notes.len(),
        clone_notes.len(),
        clone_notes
            .iter()
            .filter(|n| !logged_notes.contains(n))
            .collect::<Vec<_>>(),
        logged_notes
            .iter()
            .filter(|n| !clone_notes.contains(n))
            .collect::<Vec<_>>()
    );
    assertions::trigger_notes(&logged_notes, &clone_notes).expect("Trigger/Note parity");
}
