//! The clone's stored BAGSD series against the original's (ticket #56).
//!
//! The original stored exercise 146 from run 8's no-wearer stream served by
//! `braintellect-linux emulate`, with its consumed volts of channels 0 to 3 logged from the
//! first streamed sample. `vol146.txt` holds those volts (one consumed sample per line, four
//! channels) and `b146/ed_9.xml` the decoded type-9 record. This test runs the volts through
//! the Source chain and [`StoredSeries`] and holds every series it stores (per channel the
//! eleven `00-90` bands, the three `LMH` bands and the two `LineNoise` bands, 49 points each,
//! and the `FFT-Normal-Order` vector) to the original's within the record tolerance.
//!
//! Inputs the clone does not derive are the logged ones (`session-storage.md`, "Stored series
//! (#56)"): the graph start, the per-channel 45 Hz low-pass switch sample and the stop (the
//! closing point's x).
//!
//! Ignored: the inputs stay outside Git. Run with
//! `cargo test --release --test replay_s56 -- --ignored --nocapture`; `S56_REPLAY_DIR`
//! overrides the default directory (`tmp/s56` at the repository root).

mod harness;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::feedback::{
    Interference1, Interference2, SourceDesign, SourceFilters, StoredSeries, CHANNELS, RATE_HZ,
};
use braintellect_linux::storage::Channel;

use harness::{assertions, inputs, record};

/// Consumed sample the exercise's graph, and the stored series, start at.
const GRAPH_START: usize = 107_504;
/// Consumed sample the 45 Hz low-pass is switched in at, per channel (the BAGSD inputs
/// logged by the #56 capture).
const INTERFERENCE_SWITCH: [usize; CHANNELS] = [107_508, 107_507, 107_507, 107_507];

fn read(name: &str) -> String {
    inputs::read("S56_REPLAY_DIR", "tmp/s56", name)
}

#[test]
#[ignore = "inputs live outside Git (tmp/s56)"]
fn stored_band_series_match_the_original() {
    // The record's last 00-90 point is at the stop (240 016 ms).
    let stop_ms = 240_016.0;
    let samples = (stop_ms * RATE_HZ as f64 / 1000.0) as usize;
    let volts: Vec<[f64; CHANNELS]> = read("vol146.txt")
        .lines()
        .take(GRAPH_START + samples)
        .map(|l| {
            let v: Vec<f64> = l.split(' ').map(|t| t.parse().expect("volts")).collect();
            std::array::from_fn(|c| v[c])
        })
        .collect();
    assert_eq!(volts.len(), GRAPH_START + samples, "consumed samples");

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
    let mut stored = StoredSeries::new(counts_per_volt, RATE_HZ);
    for (s, x) in volts.iter().enumerate() {
        let filtered: [f64; CHANNELS] = std::array::from_fn(|c| {
            if s == INTERFERENCE_SWITCH[c] {
                source[c].set_interference1(Interference1::LowPass45, RATE_HZ);
            }
            source[c].push(x[c])
        });
        if s >= GRAPH_START {
            stored.push(&filtered);
        }
    }
    let actual: Vec<Channel> = stored
        .series()
        .into_iter()
        .enumerate()
        .map(|(c, series)| Channel {
            name: format!("Channel{}", c + 1),
            series,
        })
        .collect();
    let expected = record::as_stored(&read("b146/ed_9.xml"), &actual);
    for (channel, series, worst) in record::worst_by_series(&expected, &actual) {
        println!("{channel}\t{series}\t{worst:e}");
    }
    println!("{samples} samples");
    assertions::stored_series(&expected, &actual).expect("stored series parity");
}
