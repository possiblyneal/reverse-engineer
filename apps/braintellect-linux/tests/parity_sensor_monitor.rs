//! The pre-session sensor monitor (`live::monitor`) through the parity harness:
//! the in-memory B-2 stream in, the overlay state per FFT block out, compared with
//! the harness feed over the same Source output (`sensor-status.md`).

mod harness;

use std::sync::atomic::{AtomicBool, Ordering};

use braintellect_linux::acquisition::session::Session;
use braintellect_linux::feedback::{SourceDesign, SourceFilters};
use braintellect_linux::live::{self, SessionProperties};
use braintellect_linux::sensor::SensorStatus;
use harness::fixtures;
use harness::sensor;
use harness::transport::{frames_from_counts, ReplayTransport};

const FRAMES: usize = 7500;
const BLOCK_MS: i64 = 1024;

fn session() -> Session<ReplayTransport> {
    let rows: Vec<[i32; 8]> = fixtures::run8_eeg().into_iter().take(FRAMES).collect();
    let transport = ReplayTransport::new(fixtures::wire(), frames_from_counts(&rows));
    Session::open(transport, live::stream_config().expect("config")).expect("handshake")
}

/// The status after each 512-sample block of the Source output of the same frames,
/// through the harness feed.
fn reference(properties: &SessionProperties) -> Vec<SensorStatus> {
    let mut source = session();
    source.start(4).expect("start");
    let design = SourceDesign {
        interference1: properties.interference1,
        interference2: properties.interference2,
        low_pass_hz: properties.low_pass_hz,
        high_pass_hz: properties.high_pass_hz,
    };
    let mut filters: Vec<SourceFilters> = (0..4)
        .map(|_| SourceFilters::new(&design, 500).expect("filters"))
        .collect();
    let mut volts = Vec::new();
    while let Some(frame) = source.next_frame().expect("frame") {
        for n in 0..frame.channels[0].len() {
            volts.push(std::array::from_fn(|c| {
                filters[c].push(frame.channels[c][n] * 1e-6)
            }));
        }
    }
    sensor::run_feed(&volts)
}

#[test]
fn the_monitor_reports_the_status_of_the_filtered_stream_per_block() {
    let properties = SessionProperties::default();
    let stop = AtomicBool::new(false);
    let mut readings = 0i64;
    let mut clock = || {
        readings += 1;
        readings * BLOCK_MS
    };
    let mut seen = Vec::new();
    live::monitor(session(), &properties, &stop, &mut clock, &mut |s| {
        seen.push(s);
    });
    sensor::expect(&seen, &reference(&properties)).unwrap();
    assert_eq!(seen.len(), FRAMES * 2 / 512);
    assert!(!seen.contains(&SensorStatus::NotAvailable));
}

#[test]
fn the_monitor_ends_when_stopped() {
    let properties = SessionProperties::default();
    let stop = AtomicBool::new(false);
    let mut clock = || 0;
    let mut seen = 0;
    live::monitor(session(), &properties, &stop, &mut clock, &mut |_| {
        seen += 1;
        stop.store(true, Ordering::Relaxed);
    });
    assert_eq!(seen, 1);
}
