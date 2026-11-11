//! Acquisition layer replayed through the parity harness: the whole connect ->
//! handshake -> start -> stream -> stop flow runs over an in-memory transport fed
//! from the baseline fixtures, and its observable output (command bytes,
//! decoded microvolts, counter/validation errors) is compared to the baseline.

mod harness;

use braintellect_linux::acquisition::decode::ChannelCalibration;
use braintellect_linux::acquisition::session::{Error, Session};
use braintellect_linux::acquisition::stream::{SampleFrame, StreamConfig, StreamStats};
use harness::transport::ReplayTransport;
use harness::{assertions, channels, fixtures};

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
        family: None,
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
    assert_eq!(stats.lost_frames, 1);
}

#[test]
fn a_run_of_dropped_frames_counts_one_counter_error_and_every_frame_lost() {
    let mut bytes = stream_bytes();
    bytes.drain(32 * 5..32 * 8); // lose the frames with counters 5, 6 and 7
    let (frames, stats) = replay(vec![bytes]);
    assert_eq!(frames.len(), fixtures::expected_microvolts().len() - 3);
    assert_eq!(stats.packet_counter_errors, 1);
    assert_eq!(stats.lost_frames, 3);
}

#[test]
fn bad_sample_marker_bits_count_a_validation_error_and_drop_the_frame() {
    let mut bytes = stream_bytes();
    bytes[32 * 3 + 10] &= !3; // third byte of the first sample in frame 3
    let (frames, stats) = replay(vec![bytes]);
    assert_eq!(stats.sample_validation_errors, 1);
    assert_eq!(
        stats.packet_counter_errors, 0,
        "counter taken before the sample check"
    );
    assert!(
        frames.iter().all(|f| f.counter != 3),
        "invalid frame dropped"
    );
    assert_eq!(frames.len(), fixtures::expected_microvolts().len() - 1);
    assert_eq!(
        stats.lost_frames, 1,
        "a dropped frame is lost to the consumer"
    );
}

#[test]
fn a_backward_counter_step_is_a_resync_not_a_loss() {
    let mut bytes = stream_bytes();
    bytes[32 * 5 + 4..32 * 5 + 6].copy_from_slice(&2u16.to_be_bytes()); // 4 is followed by 2
    let (_, stats) = replay(vec![bytes]);
    assert_eq!(
        stats.packet_counter_errors, 2,
        "the step back and the jump after it"
    );
    assert_eq!(
        stats.lost_frames, 3,
        "only the jump from 2 to 6 skips frames"
    );
}

#[test]
fn stream_frames_still_arriving_before_the_first_reply_are_skipped() {
    // A B-2 left streaming (the previous client never sent Stop) keeps sending
    // frames until the initial Stop lands; the 101 reply follows them.
    let mut wire = fixtures::wire();
    let first_rx = wire
        .iter()
        .position(|e| matches!(e.dir, fixtures::Dir::Rx))
        .expect("a reply");
    let frames = stream_bytes()[..3 * 32].to_vec();
    wire[first_rx].bytes.splice(0..0, frames);
    let session = Session::open(ReplayTransport::new(wire, Vec::new()), config())
        .expect("handshake after leftover stream frames");
    assert_eq!(session.serial(), b"SCRUBBEDSN00");
}

#[test]
fn handshake_reply_for_the_wrong_request_is_rejected() {
    let mut wire = fixtures::wire();
    wire[2].bytes[8] = 0x6e; // serial reply echoes 110
    let result = Session::open(ReplayTransport::new(wire, Vec::new()), config());
    assert!(matches!(result, Err(Error::BadReply { code: 101 })));
}

#[test]
fn all_four_channels_match_the_originals_consumed_volts() {
    // The frames the original consumed from a replayed no-wearer stream, and the volts it
    // logged for channels 0 to 3 of each consumed sample (ticket #55, `consumed_*` fixtures).
    let (actual, stats) = channels::decode(fixtures::consumed_stream_chunks());
    assert_eq!(
        stats,
        StreamStats::default(),
        "no counter or validation errors"
    );
    let expected = fixtures::consumed_microvolts();
    assertions::channel_microvolts(&expected, &actual).expect("all-channel microvolts parity");
}
