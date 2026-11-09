//! The live session engine (acquisition -> Source filters -> SMART-2 graph ->
//! player -> sink -> storage) over the in-memory fixture transport. The EEG is
//! the run-8 exercise's raw counts framed as B-2 stream frames; the sink is a
//! recording fake, the bank synthetic and the store a scratch database. The
//! clock advances 8 ms per reading, so a run is deterministic.

mod harness;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use braintellect_linux::acquisition::decode::ChannelCalibration;
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::acquisition::stream::StreamConfig;
use braintellect_linux::capture::Recording;
use braintellect_linux::feedback::Interference1;
use braintellect_linux::live::{
    self, Environment, GraphStart, Health, Phase, SessionProperties, Snapshot,
};
use braintellect_linux::storage::Store;
use harness::audio::{small_bank, RecordingSink, RATE_HZ};
use harness::fixtures;
use harness::transport::{frames_from_counts, ReplayTransport};

/// Frames of the run-8 EEG in 30 s (250 frames a second).
const THIRTY_SECONDS: usize = 30 * 250;
/// Frames that warm the Source filters before the graph's first block (two samples a frame).
const WARMUP_FRAMES: usize = GraphStart::LIVE.warmup_samples / 2;

struct Outcome {
    last: Snapshot,
    reports: Vec<Snapshot>,
    sink: RecordingSink,
    store: Store,
    /// The raw RX bytes the recording transport tee'd off the link.
    captured: Vec<u8>,
}

/// A fresh store per call and per process, so neither a concurrent run of this suite
/// nor two tests sharing a helper (`first_sounding_block`) can delete another store's
/// rollback journal (SQLite `IOERR_DELETE_NOENT`).
fn scratch_store(name: &str) -> Store {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("live-{name}-{}-{call}.db", std::process::id()));
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
    let link = if stall {
        ReplayTransport::stalling
    } else {
        std::convert::identity
    };
    exercise_stepping(name, frames, link, properties, stop_when, 8)
}

/// [`exercise`] over the link `link` makes of the fixture transport, with a clock that
/// advances `step_ms` per reading.
fn exercise_stepping(
    name: &str,
    frames: usize,
    link: fn(ReplayTransport) -> ReplayTransport,
    properties: &SessionProperties,
    stop_when: Option<fn(&Snapshot) -> bool>,
    step_ms: i64,
) -> Outcome {
    let rows: Vec<[i32; 8]> = fixtures::run8_eeg().into_iter().take(frames).collect();
    let transport = link(ReplayTransport::new(
        fixtures::wire(),
        frames_from_counts(&rows),
    ));
    let config = StreamConfig {
        calibration: ChannelCalibration::new(12.0, 10.0).expect("calibration"),
        family: None,
    };
    let mut captured = Vec::new();
    let session =
        Session::open(Recording::new(transport, &mut captured), config).expect("handshake");
    let bank = small_bank();
    let store = scratch_store(name);
    let mut sink = RecordingSink::default();
    let stop = AtomicBool::new(false);
    let mut readings = -1i64;
    let mut clock = move || {
        readings += 1;
        readings * step_ms
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
            graph: GraphStart::LIVE,
            report: &mut report,
        },
    );
    Outcome {
        last,
        reports,
        sink,
        store,
        captured,
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
        WARMUP_FRAMES + THIRTY_SECONDS,
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
    assert_eq!(names, ["Band 10", "Band 80", "FFT-Normal-Order"]);
    let xs: Vec<f64> = series[0].points.iter().map(|p| p.x).collect();
    assert_eq!(xs, [5000.0, 10000.0, 15000.0, 20000.0, 25000.0, 30000.0]);
    assert!(series[0].points.iter().all(|p| p.y > 0.0));
    // The BAGSD report's spectrum is stored with the bands: 256 bins per channel.
    for channel in &record.channels {
        let fft = &channel.series[2];
        assert_eq!(fft.points.len(), 256, "{}", channel.name);
        assert!(fft.points.iter().any(|p| p.y > 0.0), "{}", channel.name);
    }
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
    assert_eq!(run.last.control, 111, "no epoch has ended in 30 s");
    // A block (4 samples, 8 ms) is 8 ms of audio whether or not a note rings.
    let blocks = (THIRTY_SECONDS - WARMUP_FRAMES) / 2;
    assert_eq!(run.sink.frames.len(), blocks * (RATE_HZ as usize / 125));
    assert!(run.sink.frames.iter().any(|&f| f != 0));
}

/// The block in which the first note sounds, on a clock that advances `step_ms` a reading.
fn first_sounding_block(step_ms: i64) -> usize {
    let run = exercise_stepping(
        "first-fire",
        THIRTY_SECONDS,
        std::convert::identity,
        &SessionProperties::default(),
        None,
        step_ms,
    );
    let frames_per_block = RATE_HZ as usize / 125;
    run.sink
        .frames
        .iter()
        .position(|&f| f != 0)
        .map(|at| at / frames_per_block)
        .expect("a note sounds")
}

/// 125 s of the run-8 EEG after the warm-up, streamed from its first sample: the Evaluator leaves 111
/// for the code it decides there, which the snapshot reports. The code is the clone's own on
/// this stream (the original never ran it, so no logged code applies; `replay_s48` holds the
/// parity against the original's codes).
#[test]
fn the_snapshot_reports_the_code_the_evaluator_decides() {
    let run = exercise(
        "evaluator",
        WARMUP_FRAMES + 125 * 250,
        false,
        &SessionProperties::default(),
        None,
    );

    assert_eq!(run.last.control, 12);
    // The code is reported in the frame that closes the deciding epoch's last block.
    let first = run
        .reports
        .iter()
        .find(|s| s.control != 111)
        .expect("a code change");
    assert_eq!(
        (first.frames, first.control),
        ((WARMUP_FRAMES + 2 * 15_000) as u64, 12)
    );
}

/// Every trigger is reported with its note in the frame that closes its block, so the
/// session window and a replay see each one when it happens.
#[test]
fn every_trigger_is_reported_as_it_happens() {
    let run = exercise(
        "each-trigger",
        THIRTY_SECONDS,
        false,
        &SessionProperties::default(),
        None,
    );

    let mut seen: Vec<u32> = run.reports.iter().map(|s| s.triggers).collect();
    seen.dedup();
    assert_eq!(seen, (0..=run.last.triggers).collect::<Vec<_>>());
    let first = run
        .reports
        .iter()
        .find(|s| s.triggers == 1)
        .expect("a trigger");
    let block = first_sounding_block(8);
    assert_eq!(first.frames, (WARMUP_FRAMES + 2 * (block + 1)) as u64);
}

/// The original's counter timer starts long before the first graph block (3707 to 21018 ms
/// before it in three logged runs; `audio-feedback.md`, "Counter timer start"), so the first
/// block is already past the 500 ms epoch and the first pass fires. At 8 ms a reading the
/// first pass is past one epoch anyway; at 1 ms a reading the first 500 blocks fall inside the
/// epoch of a timer started when the graph is built, which would hold the first fire back to
/// block 500. The first fire must be the same block on both clocks.
#[test]
fn the_first_counter_fire_does_not_wait_one_epoch_after_the_graph_is_built() {
    assert_eq!(first_sounding_block(1), first_sounding_block(8));
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

/// A read timeout before each frame of the warm-up, 16 ms of clock apart: frames keep
/// arriving, so the link never reads as quiet, though no block has read the clock yet.
#[test]
fn frames_arriving_between_read_timeouts_in_the_warm_up_are_not_no_data() {
    let run = exercise_stepping(
        "late",
        WARMUP_FRAMES,
        ReplayTransport::late,
        &SessionProperties::default(),
        None,
        16,
    );

    assert_eq!(run.last.frames, WARMUP_FRAMES as u64);
    assert!(run.reports.iter().all(|s| s.health != Health::NoData));
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

#[test]
fn a_session_records_every_byte_the_device_sent_handshake_included() {
    let run = exercise(
        "capture",
        THIRTY_SECONDS,
        false,
        &SessionProperties::default(),
        None,
    );

    let replies = fixtures::wire()
        .into_iter()
        .filter(|e| e.dir == fixtures::Dir::Rx)
        .flat_map(|e| e.bytes);
    let rows: Vec<[i32; 8]> = fixtures::run8_eeg()
        .into_iter()
        .take(THIRTY_SECONDS)
        .collect();
    let stream = frames_from_counts(&rows).concat();
    let expected: Vec<u8> = replies.chain(stream).collect();
    assert_eq!(run.captured, expected);
}

#[test]
fn a_capture_file_is_new_and_under_the_data_directory() {
    let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("capture-data");
    let _ = std::fs::remove_dir_all(&data);

    let (mut file, path) = braintellect_linux::capture::create(&data).expect("capture file");
    std::io::Write::write_all(&mut file, b"raw").expect("write");

    assert_eq!(path.parent(), Some(data.join("captures").as_path()));
    assert_eq!(std::fs::read(&path).expect("read back"), b"raw");
}
