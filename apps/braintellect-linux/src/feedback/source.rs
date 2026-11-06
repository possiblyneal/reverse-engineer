//! The `BSTAmplifierDataProcessor` software filter chain that sits between the
//! raw B-2 samples and the `Source` element's outputs (`processData` `0x555c00`,
//! channel filters applied by `0x557f00` in this order): interference filter
//! (`"45Hz LP"`, a 256-tap FIR, or off), interference filter 2 (25 Hz band-stop),
//! low-pass (150 Hz FIR), high-pass (1.5 Hz IIR).

use super::fir::{lowpass_taps, Fir};
use super::iir::{Iir, Notch};

/// Taps of the low-pass FIR (`setLPFilter` `0x554ae0`).
const LOW_PASS_TAPS: usize = 24;

/// Taps and edge of the `"45Hz LP"` interference filter (`setInterferenceFilter`
/// `0x554fe0`; the live run-5 capture reads 256 taps and an edge of 47.5 Hz).
const INTERFERENCE_LOW_PASS_TAPS: usize = 256;
const INTERFERENCE_LOW_PASS_HZ: f64 = 47.5;

/// The device setting `interferenceFilter` (the `"60 Hz"`, `"50 Hz"` and
/// `"50/60 Hz"` settings are not modelled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interference1 {
    Off,
    LowPass45,
}

/// The device setting `interferenceFilter2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interference2 {
    Off,
    Hz25,
}

/// The device filter settings of the session header and `app.log`.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDesign {
    pub interference1: Interference1,
    pub interference2: Interference2,
    pub low_pass_hz: f64,
    pub high_pass_hz: f64,
}

/// Round to the `f32` the app stores between stages.
fn to_f32(x: f64) -> f64 {
    f64::from(x as f32)
}

/// A fresh (zero-history) interference filter for `setting`.
fn interference_filter(setting: Interference1, rate: f64) -> Option<Fir> {
    match setting {
        Interference1::Off => None,
        Interference1::LowPass45 => Some(Fir::new(lowpass_taps(
            INTERFERENCE_LOW_PASS_HZ / rate,
            INTERFERENCE_LOW_PASS_TAPS,
        ))),
    }
}

/// One channel's filter chain.
pub struct SourceFilters {
    interference_low_pass: Option<Fir>,
    notch: Option<Notch>,
    low_pass: Fir,
    high_pass: Iir,
}

impl SourceFilters {
    pub fn new(design: &SourceDesign, rate_hz: usize) -> Self {
        let rate = rate_hz as f64;
        Self {
            interference_low_pass: interference_filter(design.interference1, rate),
            notch: match design.interference2 {
                Interference2::Off => None,
                Interference2::Hz25 => Some(Notch::hz25()),
            },
            low_pass: Fir::new(lowpass_taps(design.low_pass_hz / rate, LOW_PASS_TAPS)),
            high_pass: Iir::highpass(design.high_pass_hz, rate),
        }
    }

    /// Change the interference filter mid-stream, as `setInterferenceFilter`
    /// (`0x554fe0`) does: the new filter starts with zero history while the
    /// band-stop, low-pass and high-pass keep theirs.
    pub fn set_interference1(&mut self, setting: Interference1, rate_hz: usize) {
        self.interference_low_pass = interference_filter(setting, rate_hz as f64);
    }

    pub fn push(&mut self, x: f64) -> f64 {
        self.push_stages(x)[2]
    }

    /// The outputs of the band-stop (the input when it is off), the low-pass and the
    /// high-pass for one sample. The app holds every stage's samples as `f32` while
    /// the IPP filters compute in `f64`, so each stage rounds its output to `f32`.
    pub fn push_stages(&mut self, x: f64) -> [f64; 3] {
        let x = to_f32(x);
        let x = self
            .interference_low_pass
            .as_mut()
            .map_or(x, |fir| to_f32(fir.push(x)));
        let notched = self.notch.as_mut().map_or(x, |notch| to_f32(notch.push(x)));
        let low = to_f32(self.low_pass.push(notched));
        [notched, low, to_f32(self.high_pass.push(low))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smart2() -> SourceDesign {
        SourceDesign {
            interference1: Interference1::Off,
            interference2: Interference2::Hz25,
            low_pass_hz: 150.0,
            high_pass_hz: 1.5,
        }
    }

    #[test]
    fn switching_in_the_interference_filter_starts_it_cold_and_keeps_the_rest_warm() {
        let input = |i: usize| (i as f64 * 0.37).sin() * 1e-4;
        let mut switched = SourceFilters::new(&smart2(), 500);
        let mut tail = SourceFilters::new(&smart2(), 500);
        let mut cold = Fir::new(lowpass_taps(
            INTERFERENCE_LOW_PASS_HZ / 500.0,
            INTERFERENCE_LOW_PASS_TAPS,
        ));
        for i in 0..1000 {
            assert_eq!(switched.push(input(i)), tail.push(input(i)));
        }
        switched.set_interference1(Interference1::LowPass45, 500);
        for i in 1000..2000 {
            let x = to_f32(input(i));
            assert_eq!(switched.push(x), tail.push(to_f32(cold.push(x))));
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
