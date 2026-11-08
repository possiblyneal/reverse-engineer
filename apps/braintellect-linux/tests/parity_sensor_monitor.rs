//! The pre-session sensor monitor (`live::monitor`) through the parity harness:
//! the in-memory B-2 stream in, the overlay state per FFT block out, compared with
//! the harness feed over the same Source output (`sensor-status.md`).

mod harness;

use std::sync::atomic::{AtomicBool, Ordering};

use braintellect_linux::acquisition::session::Session;
use braintellect_linux::feedback::{Interference1, Interference2, SourceDesign, SourceFilters};
use braintellect_linux::live::{self, SessionProperties};
use braintellect_linux::sensor::SensorStatus;
use harness::fixtures;
use harness::sensor;
use harness::transport::{frames_from_counts, ReplayTransport};

const FRAMES: usize = 7500;
/// Stream time of one frame: two samples at 500 Hz.
const FRAME_MS: i64 = 4;

fn session() -> Session<ReplayTransport> {
    session_over(std::convert::identity)
}

/// [`session`] over the link `link` makes of the fixture transport.
fn session_over(link: fn(ReplayTransport) -> ReplayTransport) -> Session<ReplayTransport> {
    let rows: Vec<[i32; 8]> = fixtures::run8_eeg().into_iter().take(FRAMES).collect();
    let transport = link(ReplayTransport::new(
        fixtures::wire(),
        frames_from_counts(&rows),
    ));
    Session::open(transport, live::stream_config().expect("config")).expect("handshake")
}

/// The status at each display tick over the Source output of the same frames (the BT-LOD
/// Source filters), through the harness feed.
fn reference(properties: &SessionProperties) -> Vec<SensorStatus> {
    let mut source = session();
    source.start(4).expect("start");
    let design = SourceDesign {
        interference1: Interference1::Off,
        interference2: Interference2::Hz25,
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
    sensor::run_ticks(&volts)
}

#[test]
fn the_monitor_reports_the_status_of_the_filtered_stream_at_each_timer_tick() {
    let properties = SessionProperties::default();
    let stop = AtomicBool::new(false);
    let mut now = 0i64;
    // Read once at the start, then once per frame.
    let mut first = true;
    let mut clock = || {
        if !std::mem::take(&mut first) {
            now += FRAME_MS;
        }
        now
    };
    let mut seen = Vec::new();
    live::monitor(session(), &properties, &stop, &mut clock, &mut |s| {
        seen.push(s);
    })
    .expect("the stream ends cleanly");
    sensor::expect(&seen, &reference(&properties)).unwrap();
    assert_eq!(seen.len(), FRAMES * FRAME_MS as usize / 60);
    assert!(!seen.contains(&SensorStatus::NotAvailable));
}

#[test]
fn the_monitor_ends_when_stopped() {
    let properties = SessionProperties::default();
    let stop = AtomicBool::new(false);
    let mut now = 0;
    let mut clock = || {
        now += FRAME_MS;
        now
    };
    let mut seen = 0;
    live::monitor(session(), &properties, &stop, &mut clock, &mut |_| {
        seen += 1;
        stop.store(true, Ordering::Relaxed);
    })
    .expect("a stop is not a failure");
    assert_eq!(seen, 1);
}

#[test]
fn a_link_that_fails_mid_check_ends_it_with_the_reason() {
    let stop = AtomicBool::new(false);
    let mut clock = || 0;
    let result = live::monitor(
        session_over(ReplayTransport::breaking),
        &SessionProperties::default(),
        &stop,
        &mut clock,
        &mut |_| {},
    );
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn the_lod_source_runs_the_band_stop_and_no_low_pass_whatever_the_session_sets() {
    let session = SessionProperties::default();
    let expected = SourceDesign {
        interference1: Interference1::Off,
        interference2: Interference2::Hz25,
        low_pass_hz: session.low_pass_hz,
        high_pass_hz: session.high_pass_hz,
    };
    assert_eq!(session.interference1, Interference1::LowPass45);
    assert_eq!(session.lod_source(), expected);
    let off = SessionProperties {
        interference2: Interference2::Off,
        ..session
    };
    assert_eq!(off.lod_source(), expected);
}
