//! The fixed-mode fit ([`FitMonitor`]) over synthetic Source output shaped as the worn captures
//! are (`sensor-status.md`, "Observed: fixed-mode fit features"): mains common to every sensor
//! that touches, in phase across a pair, and a lifted sensor's mains half its partner's and in
//! antiphase to it.

use braintellect_linux::fit::{FitMonitor, SensorFit};
use braintellect_linux::sensor::SOURCE_SAMPLES;

use SensorFit::{Good, NotTouching};

const RATE_HZ: f64 = 500.0;
const MAINS_HZ: f64 = 60.0;
/// Mains amplitude per channel in volts: FP1 about 2.7 times FP2, T3 and T4 alike, as in the
/// good fits.
const MAINS_V: [f64; 4] = [3.5e-3, 1.3e-3, 3.3e-3, 3.2e-3];
/// Independent noise per channel, in volts.
const NOISE_V: f64 = 2e-4;

/// `seconds` of Source output, `lifted` (if any) with half its partner's mains amplitude, in
/// antiphase.
fn stream(lifted: Option<usize>, seconds: usize) -> Vec<[f64; 4]> {
    // A fixed linear congruential generator, so the noise is the same on every run.
    let mut state = 0x2545_f491_u64;
    let mut noise = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 2.0 * NOISE_V
    };
    (0..seconds * SOURCE_SAMPLES)
        .map(|n| {
            let mains = (std::f64::consts::TAU * MAINS_HZ * n as f64 / RATE_HZ).sin();
            std::array::from_fn(|c| {
                let amplitude = if lifted == Some(c) {
                    -0.5 * MAINS_V[c ^ 1]
                } else {
                    MAINS_V[c]
                };
                amplitude * mains + noise()
            })
        })
        .collect()
}

/// The fit after every block of `samples`.
fn fits(samples: &[[f64; 4]]) -> Vec<[SensorFit; 4]> {
    let mut monitor = FitMonitor::new();
    samples.iter().filter_map(|s| monitor.push(s)).collect()
}

#[test]
fn a_good_fit_reads_good_in_every_block() {
    let blocks = fits(&stream(None, 10));
    assert_eq!(blocks.len(), 10);
    assert!(blocks.iter().all(|f| *f == [Good; 4]), "{blocks:?}");
}

#[test]
fn a_lifted_sensor_reads_not_touching_in_every_block_and_its_partner_good() {
    for lifted in 0..4 {
        let mut want = [Good; 4];
        want[lifted] = NotTouching;
        let blocks = fits(&stream(Some(lifted), 10));
        assert_eq!(blocks.len(), 10);
        assert!(blocks.iter().all(|f| *f == want), "{lifted}: {blocks:?}");
    }
}

#[test]
fn the_fit_is_judged_once_per_second_of_source_output() {
    let samples = stream(None, 2);
    let mut monitor = FitMonitor::new();
    let judged: Vec<usize> = samples
        .iter()
        .enumerate()
        .filter_map(|(n, s)| monitor.push(s).map(|_| n + 1))
        .collect();
    assert_eq!(judged, [SOURCE_SAMPLES, 2 * SOURCE_SAMPLES]);
}
