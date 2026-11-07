//! Self-check of the parity harness: each assertion is green on the baseline
//! itself and red on a deliberately wrong output, and the baseline fixtures are
//! scrubbed and well-formed.

mod harness;

use braintellect_linux::audio::player::AudioEvent;

use harness::fixtures::{self, Dir};
use harness::tolerance::{RECORD_EPSILON, SAMPLE_EPSILON_UV, TIMING_WINDOW_MS};
use harness::{assertions, audio};

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

fn four_channels() -> Vec<Vec<f64>> {
    (0..4)
        .map(|c| (0..6).map(|i| f64::from(c * 100 + i) * 0.37).collect())
        .collect()
}

#[test]
fn channel_microvolts_green_on_equal_red_on_one_bit_or_length() {
    let baseline = four_channels();
    assertions::channel_microvolts(&baseline, &baseline).expect("baseline vs itself");

    // One ulp on one sample of channel 2 is a mismatch, reported with its position.
    let mut one_ulp = baseline.clone();
    one_ulp[2][4] = f64::from_bits(one_ulp[2][4].to_bits() + 1);
    let err = assertions::channel_microvolts(&baseline, &one_ulp).expect_err("1 ulp is red");
    assert!(err.0.contains("channel 2 sample 4"), "{}", err.0);

    // The first differing sample in channel order is the one reported.
    let mut two = one_ulp.clone();
    two[1][5] += 1.0;
    let err = assertions::channel_microvolts(&baseline, &two).expect_err("red");
    assert!(err.0.contains("channel 1 sample 5"), "{}", err.0);

    let mut short = baseline.clone();
    short[3].pop();
    assert!(assertions::channel_microvolts(&baseline, &short).is_err());
    assert!(assertions::channel_microvolts(&baseline, &baseline[..3]).is_err());
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
fn pass_flags_green_on_baseline_red_on_wrong() {
    let baseline: Vec<bool> = fixtures::feedback_blocks()
        .iter()
        .flat_map(|b| b.passes)
        .collect();
    assert!(baseline.iter().any(|&p| p) && baseline.iter().any(|&p| !p));
    assertions::pass_flags(&baseline, &baseline).expect("baseline vs itself");

    let mut flipped = baseline.clone();
    flipped[7] = !flipped[7];
    assert!(assertions::pass_flags(&baseline, &flipped).is_err());
    assert!(assertions::pass_flags(&baseline, &baseline[1..]).is_err());
}

#[test]
fn control_codes_green_on_baseline_red_on_wrong() {
    let baseline: Vec<u32> = [111, 111, 12, 12, 82].repeat(3);
    assertions::control_codes(&baseline, &baseline).expect("baseline vs itself");

    let mut wrong = baseline.clone();
    wrong[7] = 13;
    assert!(assertions::control_codes(&baseline, &wrong).is_err());
    assert!(assertions::control_codes(&baseline, &baseline[1..]).is_err());
}

#[test]
fn float32_bits_green_on_baseline_red_on_one_ulp() {
    let baseline: Vec<f32> = [1.0, 0.973_25, 0.812_5, -0.25].repeat(3);
    assertions::float32_bits("value", &baseline, &baseline).expect("baseline vs itself");

    let mut wrong = baseline.clone();
    wrong[6] = f32::from_bits(wrong[6].to_bits() + 1);
    assert!(assertions::float32_bits("value", &baseline, &wrong).is_err());
    assert!(assertions::float32_bits("value", &baseline, &baseline[1..]).is_err());
}

#[test]
fn feedback_fixtures_are_well_formed() {
    let blocks = fixtures::feedback_blocks();
    assert!(blocks.windows(2).all(|w| w[1].time_ms >= w[0].time_ms));
    assert!(blocks.iter().all(|b| b.values.len() == b.passes.len()));
    assert!(!fixtures::feedback_decisions().is_empty());
    assert!(!fixtures::feedback_eeg().is_empty());
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

#[test]
fn notes_green_on_baseline_red_on_wrong() {
    let baseline: Vec<i32> = fixtures::smart2_note_blocks()
        .iter()
        .map(|b| b.note)
        .collect();
    assert!(baseline.windows(2).any(|w| w[0] != w[1]));
    assertions::notes(&baseline, &baseline).expect("baseline vs itself");

    let mut off_by_one = baseline.clone();
    off_by_one[7] += 1;
    assert!(assertions::notes(&baseline, &off_by_one).is_err());
    assert!(assertions::notes(&baseline, &baseline[1..]).is_err());
}

#[test]
fn audio_events_green_on_baseline_within_window_red_outside() {
    let baseline = audio::run8_events();
    assert!(baseline.len() > 10);
    assertions::audio_events(&baseline, &baseline).expect("baseline vs itself");

    let mut jittered = baseline.clone();
    jittered[1].time_ms += TIMING_WINDOW_MS as i64;
    assertions::audio_events(&baseline, &jittered).expect("inside the timing window");
    jittered[1].time_ms += 1;
    assert!(assertions::audio_events(&baseline, &jittered).is_err());

    let wrongs: [fn(&mut AudioEvent); 3] =
        [|e| e.note += 1, |e| e.sample_index += 1, |e| e.len -= 1];
    for wrong in wrongs {
        let mut changed = baseline.clone();
        wrong(&mut changed[3]);
        assert!(assertions::audio_events(&baseline, &changed).is_err());
    }
    assert!(assertions::audio_events(&baseline, &baseline[1..]).is_err());
}

#[test]
fn smart2_fixtures_are_well_formed() {
    let triggers = fixtures::smart2_trigger_blocks();
    let notes = fixtures::smart2_note_blocks();
    assert!(triggers.windows(2).all(|w| w[1].time_ms >= w[0].time_ms));
    assert!(triggers.len() <= notes.len(), "trigger span is a prefix");
    assert!(notes.iter().any(|b| b.control == 111) && notes.iter().any(|b| b.control == 182));
    let decisions = fixtures::smart2_decisions();
    assert!(decisions.iter().any(|d| d.kind == "Trigger"));
    assert!(decisions.iter().any(|d| d.kind == "Note"));
}

#[test]
fn sensor_status_green_on_baseline_red_on_wrong() {
    use braintellect_linux::sensor::SensorStatus::{AdjustFit, NotAvailable, Ready};
    use harness::sensor;

    let baseline = [Ready, Ready, AdjustFit, Ready, NotAvailable];
    sensor::expect(&baseline, &baseline).expect("baseline vs itself");

    let mut wrong_state = baseline;
    wrong_state[2] = Ready;
    assert!(sensor::expect(&baseline, &wrong_state).is_err());
    assert!(sensor::expect(&baseline, &baseline[1..]).is_err());

    // The debounce: the fault is Adjust Fit only once stateChangeDelay has passed.
    let faulty = sensor::faulty_outlier();
    let early = sensor::run(true, &[(0, faulty.clone()), (999, faulty.clone())]);
    assert!(sensor::expect(&early, &[Ready, AdjustFit]).is_err());
    let late = sensor::run(true, &[(0, faulty.clone()), (1000, faulty)]);
    sensor::expect(&late, &[Ready, AdjustFit]).expect("delay reached");
}

#[test]
fn sensor_feed_red_on_a_wrong_stream_result() {
    use braintellect_linux::sensor::SensorStatus::{AdjustFit, Ready};
    use harness::sensor;

    let healthy = sensor::run_feed(&sensor::stream(4, sensor::tones_healthy));
    sensor::expect(&healthy, &[Ready; 4]).expect("healthy feed");
    let outlier = sensor::run_feed(&sensor::stream(4, sensor::tones_outlier));
    assert!(sensor::expect(&outlier, &[Ready; 4]).is_err());
    assert!(sensor::expect(&healthy, &[Ready, Ready, Ready, AdjustFit]).is_err());
    let short = sensor::run_feed(&sensor::stream(3, sensor::tones_healthy));
    assert!(sensor::expect(&short, &[Ready; 4]).is_err());
}

#[test]
fn sensor_replay_red_on_a_wrong_level_or_fault() {
    use harness::{fixtures, sensor, tolerance::LEVEL_EPSILON_DB};

    let blocks = fixtures::sensor_blocks();
    let actual = sensor::replay_blocks(&blocks);
    sensor::expect_blocks(&actual, &blocks).expect("the clone against the original");

    let mut within = actual.clone();
    within[3].0[1].bands[2] += LEVEL_EPSILON_DB / 2.0;
    sensor::expect_blocks(&within, &blocks).expect("inside the tolerance");

    let mut wrong_band = actual.clone();
    wrong_band[3].0[1].bands[2] += LEVEL_EPSILON_DB * 2.0;
    assert!(sensor::expect_blocks(&wrong_band, &blocks).is_err());
    let mut wrong_line = actual.clone();
    wrong_line[0].0[0].line_noise = -44.7;
    assert!(sensor::expect_blocks(&wrong_line, &blocks).is_err());
    let mut wrong_fault = actual.clone();
    wrong_fault[5].1 = !wrong_fault[5].1;
    assert!(sensor::expect_blocks(&wrong_fault, &blocks).is_err());
    assert!(sensor::expect_blocks(&actual[1..], &blocks).is_err());
}
