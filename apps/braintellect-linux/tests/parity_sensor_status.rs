//! Sensor status ("Ready" / "Adjust Fit" / "Not Available") through the parity
//! harness: synthetic level streams drive each fault test (`sensor-status.md`).

mod harness;

use braintellect_linux::sensor::SensorStatus::{AdjustFit, NotAvailable, Ready};
use braintellect_linux::sensor::{SensorDesign, SensorMonitor, SOURCE_SAMPLES};
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
    // the pair test from the first block. Blocks are 500 samples (1 s) and 12 zeros.
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
fn the_ceiling_is_the_grounds_minus_10_db() {
    let updates = |levels: fn() -> sensor::Levels| [(0, levels()), (1000, levels())];
    let above = sensor::run(true, &updates(sensor::just_above_ceiling));
    sensor::expect(&above, &[Ready, AdjustFit]).unwrap();
    let below = sensor::run(true, &updates(sensor::just_below_ceiling));
    sensor::expect(&below, &[Ready, Ready]).unwrap();
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
fn the_faults_match_the_original_on_a_sensors_off_replay() {
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
    let actual = sensor::replay_blocks(&blocks);
    sensor::expect_faults(&actual, &blocks).unwrap();
    sensor::expect_levels(&actual, &blocks).unwrap();
}

#[test]
fn the_spectra_match_the_originals_dspfft_bit_for_bit() {
    // Each channel's 512-sample input of 11 blocks of the sensors-off replay, through
    // the Bartlett window, the IPP real FFT, the IPP magnitude and the scale.
    let blocks = harness::fixtures::sensor_blocks();
    sensor::expect_spectra(&sensor::replay_spectra(&blocks), &blocks).unwrap();
}

/// A monitor with the engine running.
fn running_monitor() -> SensorMonitor {
    let mut monitor = SensorMonitor::new(SensorDesign::bt_lod());
    monitor.set_engine_running(true);
    monitor
}

#[test]
fn a_block_is_500_samples_and_the_next_starts_at_the_501st() {
    let mut monitor = running_monitor();
    let completed: Vec<usize> = (0..2 * SOURCE_SAMPLES)
        .filter(|_| monitor.push(&[1e-5; 4]))
        .collect();
    assert_eq!(completed, [SOURCE_SAMPLES - 1, 2 * SOURCE_SAMPLES - 1]);
}

#[test]
fn the_display_timer_debounces_between_blocks() {
    // T4 a copy of T3 faults the pair test from the first block; no block completes
    // after it, so only the timer can carry the fault past `stateChangeDelay`.
    let mut monitor = running_monitor();
    let stream = sensor::stream(1, sensor::tones_pair);
    assert!(stream.iter().fold(false, |_, v| monitor.push(v)));
    let states: Vec<_> = [1000, 1500, 1999, 2000, 2060]
        .into_iter()
        .map(|t| monitor.tick(t))
        .collect();
    sensor::expect(&states, &[Ready, Ready, Ready, AdjustFit, AdjustFit]).unwrap();
}

#[test]
fn before_the_first_block_the_default_zero_levels_fault_the_pair_test() {
    // The original shows "Adjust Fit" about a second after the engine starts, then
    // "Ready" once block 0 has real levels.
    let mut monitor = running_monitor();
    let before: Vec<_> = [0, 500, 999, 1000, 1060]
        .into_iter()
        .map(|t| monitor.tick(t))
        .collect();
    sensor::expect(&before, &[Ready, Ready, Ready, AdjustFit, AdjustFit]).unwrap();
    let stream = sensor::stream(1, sensor::tones_healthy);
    assert!(stream.iter().fold(false, |_, v| monitor.push(v)));
    sensor::expect(&[monitor.tick(1120)], &[Ready]).unwrap();
}
