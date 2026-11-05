//! The `BSTAmplifierDataProcessor` software filter chain that sits between the
//! raw B-2 samples and the `Source` element's outputs (`processData` `0x555c00`,
//! channel filters applied by `0x557f00` in this order): interference filter 2
//! (25 Hz band-stop), low-pass (150 Hz FIR), high-pass (1.5 Hz IIR). Interference
//! filter 1 is `Off` in the SMART-2 design.

use super::fir::{lowpass_taps, Fir};
use super::iir::{Iir, NOTCH_25_HZ_EDGES};

/// Taps of the low-pass FIR (`setLPFilter` `0x554ae0`).
const LOW_PASS_TAPS: usize = 24;

/// The device setting `interferenceFilter2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interference2 {
    Off,
    Hz25,
}

/// The device filter settings of the session header and `app.log`.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDesign {
    pub interference2: Interference2,
    pub low_pass_hz: f64,
    pub high_pass_hz: f64,
}

/// One channel's filter chain.
pub struct SourceFilters {
    notch: Option<Iir>,
    low_pass: Fir,
    high_pass: Iir,
}

impl SourceFilters {
    pub fn new(design: &SourceDesign, rate_hz: usize) -> Self {
        let rate = rate_hz as f64;
        Self {
            notch: match design.interference2 {
                Interference2::Off => None,
                Interference2::Hz25 => Some(Iir::bandstop(
                    NOTCH_25_HZ_EDGES.0,
                    NOTCH_25_HZ_EDGES.1,
                    rate,
                )),
            },
            low_pass: Fir::new(lowpass_taps(design.low_pass_hz / rate, LOW_PASS_TAPS)),
            high_pass: Iir::highpass(design.high_pass_hz, rate),
        }
    }

    pub fn push(&mut self, x: f64) -> f64 {
        let x = self.notch.as_mut().map_or(x, |notch| notch.push(x));
        self.high_pass.push(self.low_pass.push(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smart2() -> SourceDesign {
        SourceDesign {
            interference2: Interference2::Hz25,
            low_pass_hz: 150.0,
            high_pass_hz: 1.5,
        }
    }

    #[test]
    fn a_constant_offset_settles_to_zero() {
        let mut chain = SourceFilters::new(&smart2(), 500);
        let settled = (0..5000).map(|_| chain.push(1e-3)).last().unwrap();
        assert!(settled.abs() < 1e-9, "{settled}");
    }

    #[test]
    fn the_chain_removes_25_hz_and_keeps_10_hz() {
        let amplitude = |f_hz: f64| {
            let mut chain = SourceFilters::new(&smart2(), 500);
            let w = 2.0 * std::f64::consts::PI * f_hz / 500.0;
            (0..10_000)
                .map(|i| chain.push((w * i as f64).sin()))
                .skip(9000)
                .fold(0.0f64, |m, y| m.max(y.abs()))
        };
        assert!(amplitude(24.6) < 1e-2);
        assert!((amplitude(10.0) - 1.0).abs() < 0.05);
    }
}
