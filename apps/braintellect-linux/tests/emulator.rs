//! The B-2 emulator over `Read + Write`: it answers the original's `!*#`
//! commands from a recorded capture and streams the recorded frames on Start.
//! The capture is the baseline wire fixture (handshake replies) followed by the
//! baseline stream frames; the expected bytes are the fixtures' own.

mod harness;

use braintellect_linux::acquisition::command::Command;
use braintellect_linux::emulate::{serve, Capture};
use harness::fixtures::{self, Dir};
use harness::peer::{ScriptedPeer, Step};

/// The baseline capture: the handshake replies as the real app received them,
/// then the baseline stream frames.
fn baseline_capture() -> Vec<u8> {
    let replies = fixtures::wire()
        .into_iter()
        .filter(|e| e.dir == Dir::Rx)
        .flat_map(|e| e.bytes);
    replies.chain(fixtures::stream_chunks().concat()).collect()
}

fn handshake_replies() -> Vec<u8> {
    fixtures::wire()
        .into_iter()
        .filter(|e| e.dir == Dir::Rx)
        .flat_map(|e| e.bytes)
        .collect()
}

/// Run the emulator against `script` with a clock that advances 1 ms per turn
/// and return what it sent.
fn emulate(script: Vec<Step>) -> Vec<u8> {
    emulate_capture(&baseline_capture(), script)
}

/// [`emulate`] over the raw capture `raw`.
fn emulate_capture(raw: &[u8], script: Vec<Step>) -> Vec<u8> {
    let capture = Capture::parse(raw);
    let mut peer = ScriptedPeer::new(script);
    let mut now = 0;
    serve(&mut peer, &capture, &mut || {
        now += 1;
        now
    })
    .expect("serve");
    peer.received
}

#[test]
fn every_command_decodes_back_from_its_wire_bytes() {
    let all = [
        Command::Request(101),
        Command::Request(138),
        Command::Stop,
        Command::SetModeNormalEeg,
        Command::SetSignalInjectionOff,
        Command::Start { channels: 4 },
        Command::PowerOff,
    ];
    for command in all {
        assert_eq!(Command::decode(&command.encode()), Some(command));
    }
    // Power-off is the seven-argument frame the app wrote at exit (b2-protocol.md).
    assert_eq!(Command::PowerOff.encode(), b"!*#48,0,1,0,0,0,0,0\r\n");
    assert_eq!(Command::decode(b"!*#999,0,0,0,0,0,0\r\n"), None);
}

#[test]
fn the_handshake_is_answered_with_the_recorded_replies_byte_for_byte() {
    // Everything the real app wrote before it started streaming: the last four
    // writes are the three stops and the start.
    let sent = fixtures::commands_sent();
    let script = sent[..sent.len() - 4]
        .iter()
        .map(|c| Step::Input(c.clone()))
        .collect();

    // Both query passes, each code answered with its own pass's reply, and
    // nothing for the stop, mode and injection setters.
    assert_eq!(emulate(script), handshake_replies());
}

fn stream_bytes() -> Vec<u8> {
    fixtures::stream_chunks().concat()
}

fn quiet(turns: usize) -> impl Iterator<Item = Step> {
    (0..turns).map(|_| Step::Quiet)
}

#[test]
fn start_streams_the_recorded_frames_at_250_frames_a_second() {
    let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
    script.extend(quiet(100));

    // 100 ms after Start: 25 frames, the capture's first 25, byte for byte.
    assert_eq!(emulate(script), stream_bytes()[..25 * 32]);
}

#[test]
fn nothing_streams_before_start() {
    assert!(emulate(quiet(100).collect()).is_empty());
}

#[test]
fn stop_and_power_off_end_the_stream_without_a_reply() {
    for ending in [Command::Stop, Command::PowerOff] {
        let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
        script.extend(quiet(40));
        script.push(Step::Input(ending.encode()));
        script.extend(quiet(100));

        // Only the 10 frames due before the ending command.
        assert_eq!(emulate(script), stream_bytes()[..10 * 32], "{ending:?}");
    }
}

#[test]
fn a_restart_resumes_where_the_stream_stopped() {
    let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
    script.extend(quiet(40));
    script.push(Step::Input(Command::Stop.encode()));
    script.extend(quiet(40));
    script.push(Step::Input(Command::Start { channels: 4 }.encode()));
    script.extend(quiet(40));

    // 10 frames, a pause, then the next 10: the counter stays continuous.
    assert_eq!(emulate(script), stream_bytes()[..20 * 32]);
}

#[test]
fn the_stream_ends_with_the_capture() {
    let total = stream_bytes().len() / 32;
    let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
    script.extend(quiet((total + 10) * 4));

    assert_eq!(emulate(script), stream_bytes());
}

#[test]
fn bytes_before_the_first_marker_are_skipped() {
    // The run-8 capture opens with 24 bytes of a half-read reply (0x03 fill).
    let mut raw = vec![3u8; 24];
    raw.extend(baseline_capture());
    let capture = Capture::parse(&raw);
    let mut peer = ScriptedPeer::new(vec![Step::Input(Command::Request(101).encode())]);
    serve(&mut peer, &capture, &mut || 0).expect("serve");

    assert_eq!(peer.received, handshake_replies()[..56]);
}

#[test]
fn frames_streamed_before_the_handshake_are_not_served() {
    // A B-2 left streaming sends frames before the replies; the session skips them.
    let mut raw = stream_bytes()[..5 * 32].to_vec();
    raw.extend(baseline_capture());
    let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
    script.extend(quiet(100));

    assert_eq!(emulate_capture(&raw, script), stream_bytes()[..25 * 32]);
}

#[test]
fn a_last_frame_followed_by_a_cut_off_marker_is_kept() {
    let mut raw = baseline_capture();
    raw.extend([0xAC, 0xBC]);
    let total = stream_bytes().len() / 32;
    let mut script = vec![Step::Input(Command::Start { channels: 4 }.encode())];
    script.extend(quiet((total + 10) * 4));

    assert_eq!(emulate_capture(&raw, script), stream_bytes());
}
