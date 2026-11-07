//! Sensor-status layer: a synthetic level stream in, the overlay state per
//! update out, compared exactly.

use braintellect_linux::sensor::{ChannelLevels, SensorDesign, SensorStatus, SensorStatusEngine};

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

/// Exact match of the overlay state sequence.
pub fn expect(actual: &[SensorStatus], expected: &[SensorStatus]) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("sensor status {actual:?}, expected {expected:?}"))
    }
}
