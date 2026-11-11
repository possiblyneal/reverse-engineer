//! The fixed-mode fit over the worn B-2 captures of #61 and #76 (`sensor-status.md`, "Observed:
//! fixed-mode fit features"), through `live::monitor`: every report after the first block must
//! show the sensor lifted in that capture as not touching and every other sensor as good. The
//! T3-T4 shorted capture has every sensor touching, so it must read good throughout.
//!
//! Ignored: the captures are session EEG and live outside Git. Run with `S77_CAPTURE_DIR` set
//! to the directory holding the eight `b2-<epoch>-<label>.rx` files, else the repository root's
//! `tmp/s61/`: `cargo test --release --test replay_s77 -- --ignored --nocapture`.

mod harness;

use std::cell::Cell;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::AtomicBool;

use braintellect_linux::acquisition::session::Session;
use braintellect_linux::fit::SensorFit::{self, Good, NotTouching};
use braintellect_linux::live::{self, SessionProperties};
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

const MARKER: [u8; 4] = [0xAC, 0xBC, 0xCC, 0x03];
/// Stream time of one 32-byte frame: two samples at 500 Hz.
const FRAME_MS: i64 = 4;

/// Each capture and the channel lifted in it (FP1, FP2, T3, T4 = 0 to 3).
const CAPTURES: [(&str, Option<usize>); 8] = [
    ("b2-1794158226-goodfit.rx", None),
    ("b2-1794347114-goodfit2.rx", None),
    ("b2-1794344293-t3t4short.rx", None),
    ("b2-1794347435-fp1off.rx", Some(0)),
    ("b2-1794347642-fp2off.rx", Some(1)),
    ("b2-1794347893-t3off.rx", Some(2)),
    ("b2-1794158615-t4off.rx", Some(3)),
    ("b2-1794348085-t4off.rx", Some(3)),
];

/// Every fit `live::monitor` reports over the capture, from its first reply marker on.
fn reported(name: &str) -> Vec<[SensorFit; 4]> {
    let raw = std::fs::read(inputs::path("S77_CAPTURE_DIR", "tmp/s61").join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let first = raw
        .windows(MARKER.len())
        .position(|w| w == MARKER)
        .expect("a reply marker");
    let session = Session::open(
        Replayed(Cursor::new(raw[first..].to_vec())),
        live::stream_config().expect("config"),
    )
    .expect("handshake");
    let now = Cell::new(0i64);
    let mut first_read = true;
    let mut clock = || {
        if !std::mem::take(&mut first_read) {
            now.set(now.get() + FRAME_MS);
        }
        now.get()
    };
    let mut fits = Vec::new();
    live::monitor(
        session,
        &SessionProperties::default(),
        &AtomicBool::new(false),
        &mut clock,
        &mut |report| fits.extend(report.fit),
    )
    .expect("the stream ends cleanly");
    fits
}

#[test]
#[ignore = "needs the #61 and #76 captures, which stay outside Git"]
fn every_capture_reads_its_lifted_sensor_as_not_touching_and_the_rest_good() {
    for (name, lifted) in CAPTURES {
        let mut want = [Good; 4];
        if let Some(c) = lifted {
            want[c] = NotTouching;
        }
        let fits = reported(name);
        // 89 blocks of 60 ms ticks each.
        assert!(fits.len() > 80 * 16, "{name}: {} reports", fits.len());
        let wrong = fits.iter().filter(|f| **f != want).count();
        assert_eq!(wrong, 0, "{name}: {wrong} of {} reports", fits.len());
        println!("{name}: {} reports, all {want:?}", fits.len());
    }
}
