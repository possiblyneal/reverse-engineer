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
    // Four channels with their own tone mixes read Ready; one channel 40 dB
    // louder at the line frequency is the outlier; two identical channels fault
    // the pair test; a stream of exact zeros leaves every level at its default,
    // so each pair reads as identical. Blocks are 512 samples (1.024 s), so a
    // fault shows Adjust Fit on its second block.
    let healthy = sensor::stream(2, sensor::tones_healthy);
    sensor::expect(&sensor::run_feed(&healthy), &[Ready, Ready]).unwrap();

    let outlier = sensor::stream(2, sensor::tones_outlier);
    sensor::expect(&sensor::run_feed(&outlier), &[Ready, AdjustFit]).unwrap();

    let ceiling = sensor::stream(2, sensor::tones_ceiling);
    sensor::expect(&sensor::run_feed(&ceiling), &[Ready, AdjustFit]).unwrap();

    let pair = sensor::stream(2, sensor::tones_pair);
    sensor::expect(&sensor::run_feed(&pair), &[Ready, AdjustFit]).unwrap();

    let none = sensor::stream(2, sensor::tones_none);
    sensor::expect(&sensor::run_feed(&none), &[Ready, AdjustFit]).unwrap();
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
