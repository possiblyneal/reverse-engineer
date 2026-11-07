//! Sensor status ("Ready" / "Adjust Fit" / "Not Available") through the parity
//! harness: synthetic level streams drive each fault test (`sensor-status.md`).

mod harness;

use braintellect_linux::sensor::SensorStatus::{AdjustFit, NotAvailable, Ready};
use harness::sensor;

#[test]
fn healthy_levels_read_ready() {
    let steps = sensor::run(true, &[(0, sensor::healthy()), (500, sensor::healthy())]);
    sensor::expect(&steps, &[Ready, Ready]).unwrap();
}

#[test]
fn a_source_stream_is_fed_through_the_fft_stage_to_the_evaluator() {
    // Four channels with their own tone mixes read Ready. The line-noise level is
    // the -120 dB floor until the mains frequency is chosen after two blocks, so a
    // channel 20 dB louder at 60 Hz or above the ceiling is a fault from the third
    // block and reads Adjust Fit on the fourth. Two identical channels, or exact
    // zeros (every level at its default, so each pair reads as identical), fault
    // the pair test from the first block. Blocks are 512 samples (1.024 s).
    let healthy = sensor::stream(4, sensor::tones_healthy);
    sensor::expect(&sensor::run_feed(&healthy), &[Ready; 4]).unwrap();

    let late = [Ready, Ready, Ready, AdjustFit];
    let outlier = sensor::stream(4, sensor::tones_outlier);
    sensor::expect(&sensor::run_feed(&outlier), &late).unwrap();

    let ceiling = sensor::stream(4, sensor::tones_ceiling);
    sensor::expect(&sensor::run_feed(&ceiling), &late).unwrap();

    let early = [Ready, AdjustFit, AdjustFit, AdjustFit];
    let pair = sensor::stream(4, sensor::tones_pair);
    sensor::expect(&sensor::run_feed(&pair), &early).unwrap();

    let none = sensor::stream(4, sensor::tones_none);
    sensor::expect(&sensor::run_feed(&none), &early).unwrap();
}

#[test]
fn engine_off_reads_not_available() {
    let steps = sensor::run(false, &[(0, sensor::faulty_outlier())]);
    sensor::expect(&steps, &[NotAvailable]).unwrap();
}

#[test]
fn each_fault_test_reaches_adjust_fit_after_the_delay_and_recovers() {
    for faulty in [
        sensor::faulty_outlier(),
        sensor::faulty_ceiling(),
        sensor::faulty_pair(),
    ] {
        let steps = sensor::run(
            true,
            &[
                (0, sensor::healthy()),
                (100, faulty.clone()),
                (1099, faulty.clone()),
                (1100, faulty.clone()),
                (1200, sensor::healthy()),
                (1300, faulty.clone()),
                (1400, faulty),
            ],
        );
        sensor::expect(
            &steps,
            &[Ready, Ready, Ready, AdjustFit, Ready, Ready, Ready],
        )
        .unwrap();
    }
}

#[test]
fn the_levels_and_faults_match_the_original_on_a_sensors_off_replay() {
    // Blocks of run 8's no-wearer stream as the original's FFT saw them: the
    // line-noise level is the -120 dB floor until two blocks have passed, then
    // the 60 Hz level; the pair test faults T3/T4 on blocks 16, 20 and 31 and
    // FP1/FP2 on blocks 58 and 77, and nothing else.
    let blocks = harness::fixtures::sensor_blocks();
    let faulted: Vec<usize> = blocks
        .iter()
        .filter(|b| b.faulted)
        .map(|b| b.index)
        .collect();
    assert_eq!(faulted, [16, 20, 31, 58, 77]);
    sensor::expect_blocks(&sensor::replay_blocks(&blocks), &blocks).unwrap();
}
