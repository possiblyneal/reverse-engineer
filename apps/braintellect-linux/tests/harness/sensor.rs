//! Sensor-status layer: a synthetic level stream in, the overlay state per
//! update out, compared exactly.

use braintellect_linux::sensor::{
    ChannelLevels, SensorDesign, SensorMonitor, SensorStatus, SensorStatusEngine, BLOCK_SAMPLES,
};

use super::fixtures::SensorBlock;
use super::tolerance::LEVEL_EPSILON_DB;

pub type Levels = [ChannelLevels; 4];

fn channel(line_noise: f32, bands: [f32; 3]) -> ChannelLevels {
    ChannelLevels {
        line_noise,
        bands: bands.to_vec(),
    }
}

/// Four channels at similar, low, uncorrelated levels: no fault.
pub fn healthy() -> Levels {
    [
        channel(-80.0, [-90.0, -85.0, -95.0]),
        channel(-79.0, [-80.0, -91.0, -87.0]),
        channel(-81.0, [-93.0, -82.0, -88.0]),
        channel(-82.0, [-84.0, -96.0, -90.0]),
    ]
}

/// One channel 20 dB above the others.
pub fn faulty_outlier() -> Levels {
    let mut l = healthy();
    l[2].line_noise = -60.0;
    l
}

/// A level above the 0.0 dB ceiling.
pub fn faulty_ceiling() -> Levels {
    let mut l = healthy();
    l[1].line_noise = 1.0;
    l
}

/// T3 and T4 reading alike at every band.
pub fn faulty_pair() -> Levels {
    let mut l = healthy();
    l[3].bands = l[2].bands.clone();
    l
}

/// The status after each `(time_ms, levels)` update with the engine `running` or not.
pub fn run(running: bool, updates: &[(i64, Levels)]) -> Vec<SensorStatus> {
    let mut engine = SensorStatusEngine::new(SensorDesign::bt_lod());
    engine.set_engine_running(running);
    updates
        .iter()
        .map(|(t, levels)| engine.update(*t, levels))
        .collect()
}

const RATE: f64 = 500.0;

/// `blocks` blocks of Source output in volts, one `[f64; 4]` per sample, from
/// `tone(channel, t_seconds)`.
pub fn stream(blocks: usize, tone: fn(usize, f64) -> f64) -> Vec<[f64; 4]> {
    (0..blocks * BLOCK_SAMPLES)
        .map(|n| {
            let t = n as f64 / RATE;
            std::array::from_fn(|c| tone(c, t))
        })
        .collect()
}

fn sine(amplitude: f64, hz: f64, t: f64) -> f64 {
    amplitude * (2.0 * std::f64::consts::PI * hz * t).sin()
}

/// Per-channel mixes of the 12, 40, 60 and 70 Hz tones, microvolt-scale, with the 60 Hz
/// level above the 70 Hz control so the mains frequency is 60 Hz.
pub fn tones_healthy(c: usize, t: f64) -> f64 {
    let k = c as f64 + 1.0;
    sine(1e-6 * k, 12.0, t)
        + sine(1e-6 * (5.0 - k), 40.0, t)
        + sine(1e-6 * (10.0 + k), 60.0, t)
        + sine(1e-6 * (1.0 + 2.0 * k), 70.0, t)
}

/// T3 with a 60 Hz tone 40 dB above the others.
pub fn tones_outlier(c: usize, t: f64) -> f64 {
    tones_healthy(c, t) + if c == 2 { sine(1e-4, 60.0, t) } else { 0.0 }
}

/// FP2 above 0 dB (a spectrum value above 1.0).
pub fn tones_ceiling(c: usize, t: f64) -> f64 {
    tones_healthy(c, t) + if c == 1 { sine(0.05, 60.0, t) } else { 0.0 }
}

/// T4 a copy of T3.
pub fn tones_pair(c: usize, t: f64) -> f64 {
    tones_healthy(if c == 3 { 2 } else { c }, t)
}

/// An open input reading exactly zero.
pub fn tones_none(_c: usize, _t: f64) -> f64 {
    0.0
}

/// The overlay state after each completed block of `samples`, with the engine
/// running and the clock at the stream time of the block's last sample.
pub fn run_feed(samples: &[[f64; 4]]) -> Vec<SensorStatus> {
    let mut monitor = SensorMonitor::new(SensorDesign::bt_lod());
    monitor.set_engine_running(true);
    samples
        .iter()
        .enumerate()
        .filter_map(|(n, volts)| monitor.push((n as i64 + 1) * 2, volts))
        .collect()
}

/// The levels and fault flag the monitor holds after each of `blocks`, fed the
/// original's FFT inputs; the clock is irrelevant to both.
pub fn replay_blocks(blocks: &[SensorBlock]) -> Vec<(Levels, bool)> {
    let mut monitor = SensorMonitor::new(SensorDesign::bt_lod());
    monitor.set_engine_running(true);
    blocks
        .iter()
        .map(|block| {
            for n in 0..BLOCK_SAMPLES {
                let volts = std::array::from_fn(|c| f64::from(block.samples[c][n]));
                monitor.push(0, &volts);
            }
            (monitor.levels().clone(), monitor.faulted())
        })
        .collect()
}

/// The levels within [`LEVEL_EPSILON_DB`] and the fault flag exactly, per block.
pub fn expect_blocks(actual: &[(Levels, bool)], blocks: &[SensorBlock]) -> Result<(), String> {
    if actual.len() != blocks.len() {
        return Err(format!(
            "{} blocks, expected {}",
            actual.len(),
            blocks.len()
        ));
    }
    for ((levels, faulted), block) in actual.iter().zip(blocks) {
        if *faulted != block.faulted {
            return Err(format!(
                "block {}: fault {faulted}, expected {}",
                block.index, block.faulted
            ));
        }
        for (c, (got, want)) in levels.iter().zip(&block.levels).enumerate() {
            let got_all = std::iter::once(got.line_noise).chain(got.bands.iter().copied());
            let want_all = std::iter::once(want.line_noise).chain(want.bands);
            for (i, (g, w)) in got_all.zip(want_all).enumerate() {
                if (g - w).abs() > LEVEL_EPSILON_DB {
                    return Err(format!(
                        "block {} channel {c} level {i}: {g} dB, expected {w} dB",
                        block.index
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Exact match of the overlay state sequence.
pub fn expect(actual: &[SensorStatus], expected: &[SensorStatus]) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("sensor status {actual:?}, expected {expected:?}"))
    }
}
