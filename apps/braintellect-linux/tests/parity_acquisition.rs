//! Acquisition layer replayed through the parity harness: the whole connect ->
//! handshake -> start -> stream -> stop flow runs over an in-memory transport fed
//! from the baseline fixtures, and its observable output (command bytes,
//! decoded microvolts, counter/validation errors) is compared to the baseline.

mod harness;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::acquisition::session::{Error, Session};
use braintellect_linux::acquisition::stream::{SampleFrame, StreamConfig, StreamStats};
use harness::transport::ReplayTransport;
use harness::{assertions, fixtures};

const STOP: &[u8] = b"!*#48,0,0,0,0,0,0\r\n";

fn config() -> StreamConfig {
    let record = fixtures::stored_record();
    assert_eq!(record.number("header.deviceFamily"), 12.0);
    StreamConfig {
        calibration: ChannelCalibration::new(
            record.number("header.deviceGain"),
            record.number("header.sensorGain"),
        )
        .expect("baseline calibration is valid"),
        family: DeviceFamily::F12,
    }
}

fn transport(stream: Vec<Vec<u8>>) -> ReplayTransport {
    ReplayTransport::new(fixtures::wire(), stream)
}

#[test]
fn connect_handshake_start_stop_writes_baseline_command_bytes() {
    let mut session = Session::open(transport(fixtures::stream_chunks()), config())
        .expect("handshake against baseline replies");
    session.start(4).expect("start");
    let transport = session.disconnect().expect("disconnect");

    let baseline = fixtures::commands_sent();
    let (sent, tail) = transport.written.split_at(baseline.len());
    assertions::command_bytes(&baseline, sent).expect("command bytes parity");
    assert_eq!(tail, [STOP.to_vec()], "disconnect stops streaming");
}

#[test]
fn handshake_reads_device_serial() {
    let session = Session::open(transport(Vec::new()), config()).expect("handshake");
    assert_eq!(session.serial(), b"SCRUBBEDSN00");
}

fn stream_bytes() -> Vec<u8> {
    fixtures::stream_chunks().concat()
}

/// Run the whole flow over `stream`, returning every decoded frame and the
/// session's final error counters.
fn replay(stream: Vec<Vec<u8>>) -> (Vec<SampleFrame>, StreamStats) {
    let mut session = Session::open(transport(stream), config()).expect("handshake");
    session.start(4).expect("start");
    let mut frames = Vec::new();
    while let Some(frame) = session.next_frame().expect("stream") {
        frames.push(frame);
    }
    (frames, session.stats())
}

#[test]
fn stream_decodes_to_baseline_microvolts_and_counters() {
    let (frames, stats) = replay(fixtures::stream_chunks());

    let expected = fixtures::expected_microvolts();
    let counters: Vec<u16> = frames.iter().map(|f| f.counter).collect();
    assert_eq!(
        counters,
        expected.iter().map(|(c, _)| *c).collect::<Vec<_>>()
    );
    // The baseline rows list channel 1's two samples first, then channel 2's, ...
    let expected_uv: Vec<f64> = expected.into_iter().flat_map(|(_, uv)| uv).collect();
    let actual_uv: Vec<f64> = frames
        .iter()
        .flat_map(|f| f.channels.into_iter().flatten())
        .collect();
    assertions::microvolts(&expected_uv, &actual_uv).expect("microvolts parity");
    assert_eq!(stats, StreamStats::default(), "baseline has no errors");
}

#[test]
fn stream_resyncs_when_chunked_arbitrarily() {
    // One byte per read: no chunk boundary lines up with a frame.
    let one_byte_chunks = stream_bytes().chunks(1).map(<[u8]>::to_vec).collect();
    let (frames, stats) = replay(one_byte_chunks);
    assert_eq!(frames.len(), fixtures::expected_microvolts().len());
    assert_eq!(stats, StreamStats::default());
}

#[test]
fn dropped_frame_counts_a_packet_counter_error() {
    let mut bytes = stream_bytes();
    bytes.drain(32 * 5..32 * 6); // lose the frame with counter 5
    let (frames, stats) = replay(vec![bytes]);
    assert_eq!(frames.len(), fixtures::expected_microvolts().len() - 1);
    assert_eq!(stats.packet_counter_errors, 1);
    assert_eq!(stats.sample_validation_errors, 0);
}

#[test]
fn bad_sample_marker_bits_count_a_validation_error_and_drop_the_frame() {
    let mut bytes = stream_bytes();
    bytes[32 * 3 + 10] &= !3; // third byte of the first sample in frame 3
    let (frames, stats) = replay(vec![bytes]);
    assert_eq!(stats.sample_validation_errors, 1);
    assert!(
        frames.iter().all(|f| f.counter != 3),
        "invalid frame dropped"
    );
    assert_eq!(frames.len(), fixtures::expected_microvolts().len() - 1);
}

#[test]
fn handshake_reply_for_the_wrong_request_is_rejected() {
    let mut wire = fixtures::wire();
    wire[2].bytes[8] = 0x6e; // serial reply echoes 110
    let result = Session::open(ReplayTransport::new(wire, Vec::new()), config());
    assert!(matches!(result, Err(Error::BadReply { code: 101 })));
}
