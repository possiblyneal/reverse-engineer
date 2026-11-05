//! Self-check of the parity harness: each assertion is green on the baseline
//! itself and red on a deliberately wrong output, and the baseline fixtures are
//! scrubbed and well-formed.

mod harness;

use harness::assertions;
use harness::fixtures::{self, Dir};
use harness::tolerance::{RECORD_EPSILON, SAMPLE_EPSILON_UV, TIMING_WINDOW_MS};

#[test]
fn microvolts_green_on_baseline_red_on_wrong() {
    let baseline: Vec<f64> = fixtures::expected_microvolts()
        .into_iter()
        .flat_map(|(_, uv)| uv)
        .collect();
    assertions::microvolts(&baseline, &baseline).expect("baseline vs itself");

    let mut off_by_epsilon = baseline.clone();
    off_by_epsilon[3] += SAMPLE_EPSILON_UV * 10.0;
    assert!(assertions::microvolts(&baseline, &off_by_epsilon).is_err());

    assert!(assertions::microvolts(&baseline, &baseline[1..]).is_err());
}

#[test]
fn command_bytes_green_on_baseline_red_on_wrong() {
    let baseline = fixtures::commands_sent();
    assert!(baseline.len() > 10, "handshake fixture is non-trivial");
    assertions::command_bytes(&baseline, &baseline).expect("baseline vs itself");

    let mut wrong = baseline.clone();
    wrong[1][5] ^= 0x01;
    assert!(assertions::command_bytes(&baseline, &wrong).is_err());
    assert!(assertions::command_bytes(&baseline, &baseline[..baseline.len() - 1]).is_err());
}

#[test]
fn decisions_green_on_baseline_within_window_red_outside() {
    let baseline = fixtures::decisions();
    assert!(!baseline.is_empty());
    assertions::decisions(&baseline, &baseline).expect("baseline vs itself");

    let mut jittered = baseline.clone();
    jittered[1].time_ms += TIMING_WINDOW_MS;
    assertions::decisions(&baseline, &jittered).expect("inside the timing window");

    jittered[1].time_ms += 1;
    assert!(assertions::decisions(&baseline, &jittered).is_err());

    let mut wrong_value = baseline.clone();
    wrong_value[1].new += 1;
    assert!(assertions::decisions(&baseline, &wrong_value).is_err());
}

#[test]
fn stored_record_green_on_baseline_red_on_wrong() {
    let baseline = fixtures::stored_record();
    assertions::stored_record(&baseline, &baseline).expect("baseline vs itself");

    let mut near = baseline.clone();
    near.0.insert(
        "series.Channel1.Band-00.0.y".into(),
        format!("{}", 2.340915441513062 + RECORD_EPSILON / 2.0),
    );
    assertions::stored_record(&baseline, &near).expect("inside the record epsilon");

    let mut far = baseline.clone();
    far.0
        .insert("series.Channel1.Band-00.0.y".into(), "2.4".into());
    assert!(assertions::stored_record(&baseline, &far).is_err());

    let mut text = baseline.clone();
    text.0.insert("header.device".into(), "B-3".into());
    assert!(assertions::stored_record(&baseline, &text).is_err());

    let mut missing = baseline.clone();
    missing.0.remove("exercise.duration");
    assert!(assertions::stored_record(&baseline, &missing).is_err());
}

#[test]
fn fixtures_are_scrubbed_and_well_formed() {
    let wire = fixtures::wire();
    let serial_reply = wire
        .iter()
        .find(|e| e.dir == Dir::Rx && e.bytes.get(8) == Some(&0x65))
        .expect("101 reply");
    // Six 2-byte pairs, each followed by 03: the ASCII placeholder "SCRUBBEDSN00".
    let placeholder: Vec<u8> = b"SCRUBBEDSN00"
        .chunks(2)
        .flat_map(|pair| [pair[0], pair[1], 0x03])
        .collect();
    assert_eq!(serial_reply.bytes[10..28], placeholder[..]);

    let counters: Vec<u16> = fixtures::stream_frames()
        .iter()
        .map(|f| f.counter)
        .collect();
    assert!(counters.windows(2).all(|w| w[1] == w[0].wrapping_add(1)));
    assert_eq!(counters.len(), fixtures::expected_microvolts().len());
}
