//! The two worn B-2 captures of ticket #61 through the sensor monitor, against the original's
//! Optimize-tab sensor status on the same captures (`sensor-status.md`, "Worn replays").
//!
//! The captures are the RX bytes the clone read from a worn B-2 ("goodfit": a good fit;
//! "t4off": T4 lifted), handshake included. The original consumed them served by
//! `braintellect-linux emulate`; `scripts/worn-sensor-extract.py` reduces its hook log to
//! `worn.tsv` (per block: the overlay state when the block's interval ended and every
//! channel's line-noise and 12, 40 and 70 Hz levels) and `skipped.txt` (the stream frames its
//! host never consumed; the stream has no counter gap or failed validation there, so the
//! clone cannot find them itself and the replay leaves them out, as `replay_live` does).
//!
//! Over the frames the original consumed, every block's levels must match to the bit and the
//! state of `live::monitor` at the end of every block interval must match the original's. The
//! clone's clock is the stream's: 4 ms a frame, block `b` complete at `(b + 1)` seconds.
//!
//! Ignored: the captures are session EEG and live outside Git. Run with
//! `S61_GOODFIT_CAPTURE`, `S61_T4OFF_CAPTURE` and `S61_REPLAY_DIR` (a directory holding
//! `goodfit/` and `t4off/` with the extract's two files) set, else the defaults under the
//! repository root's `tmp/s61/`:
//! `cargo test --release --test replay_s61 -- --ignored --nocapture`.

mod harness;

use std::cell::Cell;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::AtomicBool;

use braintellect_linux::acquisition::session::Session;
use braintellect_linux::feedback::SourceFilters;
use braintellect_linux::live::{self, SessionProperties};
use braintellect_linux::sensor::{SensorDesign, SensorMonitor, SensorStatus};
use harness::inputs;

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
/// Stream time of one frame: two samples at 500 Hz.
const FRAME_MS: i64 = 4;
const BLOCK_MS: i64 = 1000;

/// What the original showed for one block.
struct Original {
    state: SensorStatus,
    /// Per channel: line-noise level, then the 12, 40 and 70 Hz levels, as `float32` bits.
    levels: [[u32; 4]; 4],
}

fn original(dir: &str) -> (Vec<Original>, Vec<usize>) {
    let read =
        |name: &str| inputs::read("S61_REPLAY_DIR", "tmp/s61/replay", &format!("{dir}/{name}"));
    let blocks = read("worn.tsv")
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let state = match f[1] {
                "Ready" => SensorStatus::Ready,
                "Adjust Fit" => SensorStatus::AdjustFit,
                "Not Available" => SensorStatus::NotAvailable,
                other => panic!("state {other}"),
            };
            let bits = |i: usize| u32::from_str_radix(f[i], 16).expect("hex");
            Original {
                state,
                levels: std::array::from_fn(|c| std::array::from_fn(|k| bits(2 + 4 * c + k))),
            }
        })
        .collect();
    let skipped = read("skipped.txt")
        .lines()
        .map(|l| l.parse().expect("frame index"))
        .collect();
    (blocks, skipped)
}

/// The capture's handshake, then its stream frames but those the original skipped.
fn consumed_session(capture: &str, default: &str, skipped: &[usize]) -> Session<Replayed> {
    let raw = std::fs::read(inputs::path(capture, default)).expect("capture");
    let mut bytes = raw[..HANDSHAKE_LEN].to_vec();
    for (n, frame) in raw[HANDSHAKE_LEN..].chunks_exact(FRAME_LEN).enumerate() {
        if !skipped.contains(&n) {
            bytes.extend_from_slice(frame);
        }
    }
    Session::open(
        Replayed(Cursor::new(bytes)),
        live::stream_config().expect("config"),
    )
    .expect("handshake")
}

/// The monitor's levels after each block, as the bits `Original::levels` holds.
fn clone_levels(mut session: Session<Replayed>) -> Vec<[[u32; 4]; 4]> {
    let design = SessionProperties::default().lod_source();
    let mut filters: Vec<SourceFilters> = (0..4)
        .map(|_| SourceFilters::new(&design, 500).expect("filters"))
        .collect();
    session.start(4).expect("start");
    let mut monitor = SensorMonitor::new(SensorDesign::bt_lod());
    monitor.set_engine_running(true);
    let mut blocks = Vec::new();
    while let Some(frame) = session.next_frame().expect("frame") {
        for n in 0..frame.channels[0].len() {
            let volts = std::array::from_fn(|c| filters[c].push(frame.channels[c][n] * 1e-6));
            if monitor.push(&volts) {
                blocks.push(std::array::from_fn(|c| {
                    let l = &monitor.levels()[c];
                    let bands = l.bands.iter().map(|b| b.to_bits());
                    let mut all = std::iter::once(l.line_noise.to_bits()).chain(bands);
                    std::array::from_fn(|_| all.next().expect("four levels"))
                }));
            }
        }
    }
    blocks
}

/// `live::monitor`'s state at the last display tick of each block's interval.
fn clone_states(session: Session<Replayed>) -> Vec<SensorStatus> {
    let now = Cell::new(0i64);
    let mut first = true;
    let mut clock = || {
        if !std::mem::take(&mut first) {
            now.set(now.get() + FRAME_MS);
        }
        now.get()
    };
    let mut states: Vec<SensorStatus> = Vec::new();
    live::monitor(
        session,
        &SessionProperties::default(),
        &AtomicBool::new(false),
        &mut clock,
        &mut |status| {
            // Block `b` completes at `(b + 1)` seconds and is shown until the next block.
            let Ok(block) = usize::try_from(now.get() / BLOCK_MS - 1) else {
                return;
            };
            states.resize(block + 1, status);
            states[block] = status;
        },
    )
    .expect("the stream ends cleanly");
    states
}

fn check(label: &str, capture: &str) {
    let (blocks, skipped) = original(label);
    let default = format!("tmp/s61/{label}.rx");
    let session = || consumed_session(capture, &default, &skipped);

    let levels = clone_levels(session());
    assert_eq!(levels.len(), blocks.len(), "{label}: blocks");
    for (b, (got, want)) in levels.iter().zip(&blocks).enumerate() {
        assert_eq!(
            got, &want.levels,
            "{label}: block {b} levels (line noise, 12, 40, 70 Hz)"
        );
    }

    let states = clone_states(session());
    assert!(
        states.len() >= blocks.len(),
        "{label}: {} states",
        states.len()
    );
    for (b, want) in blocks.iter().enumerate() {
        assert_eq!(states[b], want.state, "{label}: block {b} state");
    }
}

#[test]
#[ignore = "needs the #61 captures and the original's extract, which stay outside Git"]
fn the_good_fit_capture_matches_the_original_in_every_block() {
    check("goodfit", "S61_GOODFIT_CAPTURE");
}

#[test]
#[ignore = "needs the #61 captures and the original's extract, which stay outside Git"]
fn the_t4_off_capture_matches_the_original_in_every_block() {
    check("t4off", "S61_T4OFF_CAPTURE");
}
