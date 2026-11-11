//! Fixed-mode fit status per sensor: each of FP1, FP2, T3 and T4 good or not touching, set
//! from the worn captures of #61 and #76, not from the BT-LOD overlay's fault tests
//! ([`crate::sensor`]).
//!
//! Each second of Source output (the [`crate::sensor::SOURCE_SAMPLES`] blocks the overlay also
//! uses) is judged per pair, FP1/FP2 and T3/T4. When the pair's two signals are negatively
//! correlated over the block, the one with the lower RMS is not touching; otherwise both are
//! good. Evidence, margins and limits: `sensor-status.md`, "Observed: fixed-mode fit features".

use crate::acquisition::stream::CHANNELS;
use crate::sensor::SOURCE_SAMPLES;

/// One sensor's fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorFit {
    Good,
    NotTouching,
}

impl SensorFit {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::NotTouching => "not touching",
        }
    }
}

/// The sensor pairs, as channel indices: FP1/FP2 and T3/T4.
const PAIRS: [(usize, usize); 2] = [(0, 1), (2, 3)];

/// Judges the fit once per block of Source output.
#[derive(Debug, Clone, Default)]
pub struct FitMonitor {
    block: Vec<[f64; CHANNELS]>,
}

impl FitMonitor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One Source sample per channel, in volts. Returns the fit of every sensor when the
    /// sample completes a block.
    pub fn push(&mut self, volts: &[f64; CHANNELS]) -> Option<[SensorFit; CHANNELS]> {
        self.block.push(*volts);
        if self.block.len() < SOURCE_SAMPLES {
            return None;
        }
        let fit = judge(&self.block);
        self.block.clear();
        Some(fit)
    }
}

fn judge(block: &[[f64; CHANNELS]]) -> [SensorFit; CHANNELS] {
    let n = block.len() as f64;
    let mean: [f64; CHANNELS] =
        std::array::from_fn(|c| block.iter().map(|s| s[c]).sum::<f64>() / n);
    let covariance = |a: usize, b: usize| {
        block
            .iter()
            .map(|s| (s[a] - mean[a]) * (s[b] - mean[b]))
            .sum::<f64>()
    };
    let mut fit = [SensorFit::Good; CHANNELS];
    for (a, b) in PAIRS {
        // The sign of the correlation is the sign of the covariance.
        if covariance(a, b) < 0.0 {
            let quieter = if covariance(a, a) < covariance(b, b) {
                a
            } else {
                b
            };
            fit[quieter] = SensorFit::NotTouching;
        }
    }
    fit
}
