//! The live session engine (acquisition -> Source filters -> SMART-2 graph ->
//! player -> sink -> storage) over the in-memory fixture transport. The EEG is
//! the run-8 exercise's raw counts framed as B-2 stream frames; the sink is a
//! recording fake, the bank synthetic and the store a scratch database. The
//! clock advances 8 ms per reading, so a run is deterministic.

mod harness;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::acquisition::stream::StreamConfig;
use braintellect_linux::feedback::Interference1;
use braintellect_linux::live::{self, Environment, Health, Phase, SessionProperties, Snapshot};
use braintellect_linux::storage::Store;
use harness::audio::{small_bank, RecordingSink, RATE_HZ};
use harness::fixtures;
use harness::transport::{frames_from_counts, ReplayTransport};

/// Frames of the run-8 EEG in 30 s (250 frames a second).
const THIRTY_SECONDS: usize = 30 * 250;

struct Outcome {
    last: Snapshot,
    reports: Vec<Snapshot>,
    sink: RecordingSink,
    store: Store,
}

fn scratch_store(name: &str) -> Store {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("live-{name}.db"));
    let _ = std::fs::remove_file(&path);
    Store::open(&path).expect("scratch store")
}

/// Run a session over `frames` frames of the run-8 EEG. `stall` leaves the link
/// quiet after the stream instead of closing it; `stop_after` raises Stop once
/// a report satisfies `stop_when`.
fn exercise(
    name: &str,
    frames: usize,
    stall: bool,
    properties: &SessionProperties,
    stop_when: Option<fn(&Snapshot) -> bool>,
) -> Outcome {
    let rows: Vec<[i32; 8]> = fixtures::run8_eeg().into_iter().take(frames).collect();
    let mut transport = ReplayTransport::new(fixtures::wire(), frames_from_counts(&rows));
    if stall {
        transport = transport.stalling();
    }
    let config = StreamConfig {
        calibration: ChannelCalibration::new(12.0, 10.0).expect("calibration"),
        family: DeviceFamily::F12,
    };
    let session = Session::open(transport, config).expect("handshake");
    let bank = small_bank();
    let store = scratch_store(name);
    let mut sink = RecordingSink::default();
    let stop = AtomicBool::new(false);
    let mut readings = -1i64;
    let mut clock = move || {
        readings += 1;
        readings * 8
    };
    let mut reports: Vec<Snapshot> = Vec::new();
    let mut report = |s: &Snapshot| {
        reports.push(s.clone());
        if stop_when.is_some_and(|f| f(s)) {
            stop.store(true, Ordering::Relaxed);
        }
    };
    let last = live::run(
        session,
        properties,
        Environment {
            bank: &bank,
            sink: &mut sink,
            store: &store,
            stop: &stop,
            clock: &mut clock,
            report: &mut report,
        },
    );
    Outcome {
        last,
        reports,
        sink,
        store,
    }
}

fn saved_id(run: &Outcome) -> i64 {
    match run.last.phase {
        Phase::Saved { id } => id,
        ref other => panic!("expected a stored session, got {other:?}"),
    }
}

#[test]
fn a_run_to_the_end_of_the_stream_is_stored_and_readable() {
    let run = exercise(
        "end",
        THIRTY_SECONDS,
        false,
        &SessionProperties::default(),
        None,
    );

    let record = run.store.load(saved_id(&run)).expect("stored record");
    assert_eq!(record.duration_s, 30);
    assert_eq!(record.header.design, "BT-SMART-2-v5.1.2");
    assert_eq!(record.header.number_channels, 4);
    assert_eq!(record.header.device_low_pass_cutoff, 150.0);
    assert_eq!(record.channels.len(), 4);
    // One point per channel and band every 5 s, as the baseline record keeps.
    let series = &record.channels[0].series;
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Band 10", "Band 80"]);
    let xs: Vec<f64> = series[0].points.iter().map(|p| p.x).collect();
    assert_eq!(xs, [5000.0, 10000.0, 15000.0, 20000.0, 25000.0, 30000.0]);
    assert!(series[0].points.iter().all(|p| p.y > 0.0));
}

#[test]
fn status_goes_from_waiting_to_streaming_and_counts_frames() {
    let run = exercise(
        "status",
        THIRTY_SECONDS,
        false,
        &SessionProperties::default(),
        None,
    );

    assert_eq!(run.reports[0].health, Health::WaitingForData);
    assert_eq!(run.reports[0].frames, 0);
    assert!(run
        .reports
        .iter()
        .any(|s| s.phase == Phase::Running && s.health == Health::Streaming));
    assert_eq!(run.last.frames, THIRTY_SECONDS as u64);
    assert_eq!(run.last.stats.packet_counter_errors, 0);
    assert_eq!(run.last.stats.sample_validation_errors, 0);
    // The signal level is live: every channel reads a non-zero RMS in microvolts.
    assert!(run.last.rms_uv.iter().all(|&v| v > 0.0));
}

#[test]
fn triggers_reach_the_sink_as_audio_and_are_counted() {
    let run = exercise(
        "sound",
        THIRTY_SECONDS,
        false,
        &SessionProperties::default(),
        None,
    );

    assert!(
        run.last.triggers > 0,
        "the run-8 EEG triggers under control 111"
    );
    assert!(run.last.last_note.is_some_and(|n| (40..=76).contains(&n)));
    // A block (4 samples, 8 ms) is 8 ms of audio whether or not a note rings.
    let blocks = THIRTY_SECONDS / 2;
    assert_eq!(run.sink.frames.len(), blocks * (RATE_HZ as usize / 125));
    assert!(run.sink.frames.iter().any(|&f| f != 0));
}

#[test]
fn stop_ends_a_quiet_link_and_still_stores_the_session() {
    let run = exercise(
        "stop",
        1000,
        true,
        &SessionProperties::default(),
        Some(|s| s.health == Health::NoData),
    );

    assert_eq!(run.last.frames, 1000);
    saved_id(&run);
}

#[test]
fn the_session_length_ends_the_run() {
    let properties = SessionProperties {
        duration_s: 10,
        ..SessionProperties::default()
    };
    let run = exercise("length", THIRTY_SECONDS, true, &properties, None);

    assert_eq!(
        run.store.load(saved_id(&run)).expect("record").duration_s,
        10
    );
}

#[test]
fn the_clinicians_filter_settings_are_stored_in_the_header() {
    let properties = SessionProperties {
        interference1: Interference1::Off,
        low_pass_hz: 100.0,
        high_pass_hz: 0.5,
        ..SessionProperties::default()
    };
    let run = exercise("settings", 500, false, &properties, None);

    let header = run.store.load(saved_id(&run)).expect("record").header;
    assert_eq!(header.interference_filter, "Off");
    assert_eq!(header.device_low_pass_cutoff, 100.0);
    assert_eq!(header.device_high_pass_cutoff, 0.5);
}

#[test]
fn a_quiet_link_reads_as_no_data_and_stores_nothing() {
    let properties = SessionProperties {
        duration_s: 3,
        ..SessionProperties::default()
    };
    let run = exercise("quiet", 0, true, &properties, None);

    assert!(run.reports.iter().any(|s| s.health == Health::NoData));
    assert!(matches!(run.last.phase, Phase::Failed(_)));
    assert!(run.store.list().expect("list").is_empty());
}
