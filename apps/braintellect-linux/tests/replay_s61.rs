//! The three worn B-2 captures of ticket #61 through the sensor monitor, against the original's
//! Optimize-tab sensor status on the same captures (`sensor-status.md`, "Worn replays").
//!
//! The captures are the RX bytes the clone read from a worn B-2 ("goodfit": a good fit; "t4off": T4
//! lifted; "t3t4short": the T3 and T4 inputs shorted together), handshake included. The original
//! consumed them served by `braintellect-linux emulate`; `scripts/worn-sensor-extract.py` reduces
//! its hook log to `worn.tsv` (per block: the overlay state when the block's interval ended and
//! every channel's line-noise and 12, 40 and 70 Hz levels) and `skipped.txt` (the stream frames its
//! host never consumed; the stream has no counter gap or failed validation there, so the clone
//! cannot find them itself and the replay leaves them out, as `replay_live` does).
//!
//! The no-wearer stream of run 8 (#57) is replayed the same way over the Optimize-tab period
//! before its Play, with its own extract.
//!
//! A fault that lasts one block reaches the 1000 ms debounce delay at the very tick (16 ticks
//! of 62.5 ms) at which the next block's update clears it. The original showed red for the
//! block in 2 of 3 such cases of run 8 and not in the third; which came first, the display
//! timer or the engine thread, is host timing. `ties.txt` lists the blocks the tick won (the
//! delay reached in the same clock reading as, and logged before, the next block's update) and
//! their state is not compared; a block the update won is compared.
//!
//! Over the frames the original consumed, every block's levels must match to the bit and the
//! state of `live::monitor` at the end of every block interval must match the original's. The
//! clone's clock is the stream's: 4 ms a frame, block `b` complete at `(b + 1)` seconds.
//!
//! Ignored: the captures are session EEG and live outside Git. Run with
//! `S61_GOODFIT_CAPTURE`, `S61_T4OFF_CAPTURE`, `S61_SHORT_CAPTURE` and `S61_REPLAY_DIR` (a
//! directory holding `goodfit/`, `t4off/` and `t3t4short/` with the extract's files) set, else
//! the defaults under the repository root's `tmp/s61/`; run 8 takes `S57_RUN8_CAPTURE` and
//! `S57_REPLAY_DIR` (holding `run8/`, default `tmp/s57o/replay`):
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
const MARKER: [u8; 4] = [0xAC, 0xBC, 0xCC, 0x03];
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

/// One capture and where the original's extract of it lives. Paths come from the environment
/// variable, else the default under the repository root.
struct Case {
    /// The extract's folder inside the replay directory.
    label: &'static str,
    capture: (&'static str, &'static str),
    replay: (&'static str, &'static str),
    /// The capture streams on past the period the original's log covers (run 8 goes on into
    /// its Play), so the replay stops two seconds past the last logged block.
    outlasts_log: bool,
}

/// The original's blocks, the stream frames its host skipped and the blocks whose state is a
/// timing race (see the module docs).
fn original(case: &Case) -> (Vec<Original>, Vec<usize>, Vec<usize>) {
    let (var, default) = case.replay;
    let read = |name: &str| inputs::read(var, default, &format!("{}/{name}", case.label));
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
    let ties = read("ties.txt")
        .lines()
        .map(|l| l.parse().expect("block index"))
        .collect();
    (blocks, skipped, ties)
}

/// The capture's handshake, then its stream frames but those the original skipped, up to
/// `frames` of them (a capture can outlast the original's log). The handshake starts at the
/// first reply marker; bytes before it are a partial read.
fn consumed_session(case: &Case, skipped: &[usize], frames: usize) -> Session<Replayed> {
    let raw = std::fs::read(inputs::path(case.capture.0, case.capture.1)).expect("capture");
    let first = raw
        .windows(MARKER.len())
        .position(|w| w == MARKER)
        .expect("a reply marker");
    let mut bytes = raw[first..first + HANDSHAKE_LEN].to_vec();
    let stream = raw[first + HANDSHAKE_LEN..].chunks_exact(FRAME_LEN);
    for (n, frame) in stream.enumerate().take(frames) {
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
        &mut |report| {
            let status = report.overlay;
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

fn check(case: &Case) {
    let label = case.label;
    let (blocks, skipped, ties) = original(case);
    // The state of the last block needs its whole interval; skipped frames are not streamed.
    let frames = if case.outlasts_log {
        (blocks.len() + 2) * usize::try_from(BLOCK_MS / FRAME_MS).expect("small") + skipped.len()
    } else {
        usize::MAX
    };
    let session = || consumed_session(case, &skipped, frames);

    let levels = clone_levels(session());
    if case.outlasts_log {
        assert!(
            levels.len() >= blocks.len(),
            "{label}: {} blocks",
            levels.len()
        );
    } else {
        assert_eq!(levels.len(), blocks.len(), "{label}: blocks");
    }
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
    for (b, want) in blocks.iter().enumerate().filter(|(b, _)| !ties.contains(b)) {
        assert_eq!(states[b], want.state, "{label}: block {b} state");
    }
}

const WORN: (&str, &str) = ("S61_REPLAY_DIR", "tmp/s61/replay");

#[test]
#[ignore = "needs the #61 captures and the original's extract, which stay outside Git"]
fn the_good_fit_capture_matches_the_original_in_every_block() {
    check(&Case {
        label: "goodfit",
        capture: ("S61_GOODFIT_CAPTURE", "tmp/s61/goodfit.rx"),
        replay: WORN,
        outlasts_log: false,
    });
}

#[test]
#[ignore = "needs the #61 captures and the original's extract, which stay outside Git"]
fn the_t4_off_capture_matches_the_original_in_every_block() {
    check(&Case {
        label: "t4off",
        capture: ("S61_T4OFF_CAPTURE", "tmp/s61/t4off.rx"),
        replay: WORN,
        outlasts_log: false,
    });
}

#[test]
#[ignore = "needs the #61 captures and the original's extract, which stay outside Git"]
fn the_t3_t4_shorted_capture_matches_the_original_in_every_block() {
    check(&Case {
        label: "t3t4short",
        capture: ("S61_SHORT_CAPTURE", "tmp/s61/t3t4short.rx"),
        replay: WORN,
        outlasts_log: false,
    });
}

/// Run 8's stream, no wearer, over the Optimize-tab period before its Play (#57): the original
/// logged 97 blocks and its host skipped 14 stream frames.
#[test]
#[ignore = "needs run 8's raw stream and the original's extract, which stay outside Git"]
fn the_no_wearer_run_8_stream_matches_the_original_in_every_block() {
    check(&Case {
        label: "run8",
        capture: ("S57_RUN8_CAPTURE", "tmp/s48/rx8.raw"),
        replay: ("S57_REPLAY_DIR", "tmp/s57o/replay"),
        outlasts_log: true,
    });
}
