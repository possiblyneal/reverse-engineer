//! Acquisition layer replayed through the parity harness: the clone's sample
//! decode and ADC->volts conversion must reproduce the microvolts the baseline
//! stream carries.

mod harness;

use braintellect_linux::acquisition::decode::{
    decode_sample_i24_be, ChannelCalibration, DeviceFamily,
};
use harness::{assertions, fixtures};

#[test]
fn decoded_microvolts_match_baseline() {
    let record = fixtures::stored_record();
    let calibration = ChannelCalibration::new(
        record.number("header.deviceGain"),
        record.number("header.sensorGain"),
    )
    .expect("baseline calibration is valid");
    assert_eq!(record.number("header.deviceFamily"), 12.0);

    let frames = fixtures::stream_frames();
    let actual: Vec<f64> = frames
        .iter()
        .flat_map(|frame| frame.payload.chunks_exact(3))
        .map(|s| {
            let raw = decode_sample_i24_be([s[0], s[1], s[2]]);
            calibration.to_volts(raw, DeviceFamily::F12) * 1e6
        })
        .collect();
    let expected: Vec<f64> = fixtures::expected_microvolts()
        .into_iter()
        .flat_map(|(_, uv)| uv)
        .collect();

    assertions::microvolts(&expected, &actual).expect("microvolts parity");
}
