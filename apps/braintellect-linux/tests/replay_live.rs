//! The #33 live session's raw capture replayed through the live engine (`live::run`) over an
//! in-memory transport, against the original's run on the same capture (ticket #60).
//!
//! The capture is the RX bytes the clone read from the B-2 in the #33 session, handshake
//! replies included, so reading it back in order replays that session; writes are dropped.
//! The original consumed the same capture served by `braintellect-linux emulate`; its hook log
//! is reduced by `scripts/replay-extract.py` to `blocks.tsv` (the code of every block) and
//! `decisions.tsv` (its Trigger and Note decisions).
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
use braintellect_linux::feedback::DEFAULT_CODE;
use braintellect_linux::live::{self, Environment, Phase, SessionProperties, Snapshot};
use braintellect_linux::storage::Store;
use harness::assertions;
use harness::audio::{small_bank, RecordingSink};

fn path(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(default)
        },
        PathBuf::from,
    )
}

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
    let mut frames = stream.chunks_exact(FRAME_LEN);
    samples
        .chunks_exact(2)
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
    let raw = fs::read(path("S33_CAPTURE", "tmp/s33/b2.rx")).expect("capture");
    let dir = path("S33_REPLAY_DIR", "tmp/s33/replay");
    let read = |name: &str| fs::read_to_string(dir.join(name)).expect(name);

    let read_ms: Vec<i64> = read("blocks.tsv")
        .lines()
        .map(|l| l.split('\t').nth(2).expect("read").parse().expect("read"))
        .collect();
    let stream = consumed_frames(&raw[HANDSHAKE_LEN..], &read("eeg.txt"));
    let mut bytes = raw[..HANDSHAKE_LEN].to_vec();
    bytes.extend(stream[GRAPH_START / 2..].concat());
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
    // The graph is built on the first read, then one read per block: the original's.
    let mut reads = std::iter::once(read_ms[0]).chain(read_ms.iter().copied());
    let mut clock = move || reads.next().expect("a logged read per block");
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
            report: &mut |s: &Snapshot| reports.push(s.clone()),
        },
    );
    assert!(
        matches!(last.phase, Phase::Saved { .. }),
        "{:?}",
        last.phase
    );

    let logged: Vec<u32> = read("blocks.tsv")
        .lines()
        .map(|l| l.split('\t').nth(1).expect("code").parse().expect("code"))
        .collect();
    let logged_notes: Vec<(i64, u32)> = read("decisions.tsv")
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[1] == "Note").then(|| (f[0].parse().expect("time"), f[2].parse().expect("note")))
        })
        .collect();
    // A code change or a trigger is reported in the frame that closes its block.
    let block_of = |s: &Snapshot| usize::try_from(s.frames / 2).expect("blocks") - 1;
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
            clone_notes.push((read_ms[block_of(s)], note));
        }
    }
    assert_eq!(
        last.frames / 2,
        logged.len() as u64,
        "every logged block ran"
    );
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

    // Trigger and Note are reported, not asserted: the live engine starts its Source chain
    // cold at the graph start and runs its own clock and sum-order policies, where the
    // original warmed the chain on the pre-session stream (`audio-feedback.md`, "Live
    // replay of the #33 capture").
    let same: Vec<bool> = clone_notes
        .iter()
        .map(|n| logged_notes.contains(n))
        .collect();
    println!(
        "notes (read ms, note): original {}, clone {}, identical {}; clone only {:?}; original only {:?}",
        logged_notes.len(),
        clone_notes.len(),
        same.iter().filter(|&&s| s).count(),
        clone_notes
            .iter()
            .zip(&same)
            .filter(|(_, &s)| !s)
            .map(|(n, _)| n)
            .collect::<Vec<_>>(),
        logged_notes
            .iter()
            .filter(|n| !clone_notes.contains(n))
            .collect::<Vec<_>>()
    );
}
