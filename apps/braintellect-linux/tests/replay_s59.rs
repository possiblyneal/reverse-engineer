//! Reports the clone's BAGSD discrete-FFT spectra against the original's stored
//! `FFT-Normal-Order` vectors of exercise 146 (ticket #59). The comparison is reported, not
//! asserted: the clone does not match yet (`bagsd-design.md`, "Discrete FFT"); the day it does,
//! replace the `println!` with `assertions::fft_spectra(...).expect(...)`.
//!
//! The capture is #56's: `vol146.txt` holds the consumed volts (one sample per line, four
//! channels) and `b146/ed_9.xml` the decoded type-9 record. Both are vendor-derived and stay
//! under `tmp/`. Run with `cargo test --release --test replay_s59 -- --ignored --nocapture`;
//! `S59_REPLAY_DIR` overrides the default directory (`tmp/s56` at the repository root).

mod harness;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::feedback::{
    FftSpectrum, Interference1, Interference2, SourceDesign, SourceFilters, CHANNELS, RATE_HZ,
};
use harness::inputs;

/// Consumed sample the exercise's graph starts at, and its length: 240 016 ms.
const GRAPH_START: usize = 107_504;
const SAMPLES: usize = 120_008;
/// Consumed sample the 45 Hz low-pass is switched in at, per channel.
const INTERFERENCE_SWITCH: [usize; CHANNELS] = [107_508, 107_507, 107_507, 107_507];

fn read(name: &str) -> String {
    inputs::read("S59_REPLAY_DIR", "tmp/s56", name)
}

/// The `FFT-Normal-Order` vector of each channel, in microvolts.
fn expected() -> Vec<Vec<f64>> {
    read("b146/ed_9.xml")
        .split("<base:fftDataList")
        .skip(1)
        .map(|block| {
            block
                .split("</base:fftDataList>")
                .next()
                .expect("closed list")
                .split("<base:yValue>")
                .skip(1)
                .map(|t| t.split('<').next().expect("value").parse().expect("y"))
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "inputs live outside Git (tmp/s56)"]
fn report_discrete_fft_against_the_stored_spectra() {
    let design = SourceDesign {
        interference1: Interference1::Off,
        interference2: Interference2::Hz25,
        low_pass_hz: 150.0,
        high_pass_hz: 1.5,
    };
    let mut source: [SourceFilters; CHANNELS] =
        std::array::from_fn(|_| SourceFilters::new(&design, RATE_HZ).expect("design"));
    let counts_per_volt = ChannelCalibration::new(12.0, 10.0)
        .expect("baseline calibration")
        .divisor(DeviceFamily::F12);
    let mut fft: [FftSpectrum; CHANNELS] =
        std::array::from_fn(|_| FftSpectrum::new(counts_per_volt));
    for (s, line) in read("vol146.txt")
        .lines()
        .take(GRAPH_START + SAMPLES)
        .enumerate()
    {
        let volts: Vec<f64> = line.split(' ').map(|t| t.parse().expect("volts")).collect();
        for c in 0..CHANNELS {
            if s == INTERFERENCE_SWITCH[c] {
                source[c].set_interference1(Interference1::LowPass45, RATE_HZ);
            }
            let filtered = source[c].push(volts[c]);
            if s >= GRAPH_START {
                fft[c].push(filtered);
            }
        }
    }
    let actual: Vec<Vec<f64>> = fft.into_iter().map(FftSpectrum::finish).collect();
    let expected = expected();
    for (c, (e, a)) in expected.iter().zip(&actual).enumerate() {
        let worst = e
            .iter()
            .zip(a)
            .map(|(e, a)| (e - a).abs() / e)
            .fold(0.0, f64::max);
        let low = (0..49).map(|k| (e[k] - a[k]).abs() / e[k]).sum::<f64>() / 49.0;
        println!("ch{c}: mean rel dev bins 0..48 {low:.4}, worst rel dev {worst:.3}");
    }
    assert_eq!(expected.len(), actual.len(), "channels");
}
