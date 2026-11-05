//! `BandPassFilter` `OutAmplitude` (vtable `0x18bf7fc`, rebuild `0x66dc00`,
//! per-block process `0x66e9f0`; `audio-feedback.md`, "BandPassFilter
//! OutAmplitude"): a FIR band-pass, the sliding maximum of its absolute value over
//! `W` samples, then a 16-tap mean scaled by 2.

use std::collections::VecDeque;

use super::fir::{bandpass_taps, Fir};

/// Taps of the final mean (`0x6507e0`), each `2.0 / 16` (constant `0x18953a8`).
const MEAN_TAPS: usize = 16;
const MEAN_GAIN: f64 = 2.0 / MEAN_TAPS as f64;

/// A `BandPassFilter` element's design properties.
#[derive(Debug, Clone, PartialEq)]
pub struct BandPassDesign {
    pub low_hz: f64,
    pub high_hz: f64,
    /// `filterOrder`: the FIR's tap count.
    pub order: usize,
}

pub struct BandPass {
    fir: Fir,
    /// Sliding-maximum window `W`.
    window: usize,
    seen: usize,
    /// Monotonic queue of `(index, |y|)`, largest first.
    maxima: VecDeque<(usize, f64)>,
    means: VecDeque<f64>,
}

impl BandPass {
    /// `block_len` is the samples per stream block (`W` is at least that long).
    pub fn new(design: &BandPassDesign, rate_hz: usize, block_len: usize) -> Self {
        let rate = rate_hz as f64;
        let window = ((rate / (2.0 * design.high_hz)) as usize).max(block_len);
        Self {
            fir: Fir::new(bandpass_taps(
                design.low_hz / rate,
                design.high_hz / rate,
                design.order,
            )),
            window,
            seen: 0,
            maxima: VecDeque::new(),
            means: VecDeque::with_capacity(MEAN_TAPS + 1),
        }
    }

    /// Feed one sample (volts) and return the amplitude.
    pub fn push(&mut self, x: f64) -> f64 {
        let y = self.fir.push(x).abs();
        let i = self.seen;
        self.seen += 1;
        while self.maxima.back().is_some_and(|&(_, v)| v <= y) {
            self.maxima.pop_back();
        }
        self.maxima.push_back((i, y));
        while self
            .maxima
            .front()
            .is_some_and(|&(j, _)| j + self.window <= i)
        {
            self.maxima.pop_front();
        }
        self.means
            .push_back(self.maxima.front().expect("just pushed").1);
        if self.means.len() > MEAN_TAPS {
            self.means.pop_front();
        }
        MEAN_GAIN * self.means.iter().sum::<f64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn band_80() -> BandPassDesign {
        BandPassDesign {
            low_hz: 23.0,
            high_hz: 36.0,
            order: 128,
        }
    }

    #[test]
    fn a_centre_frequency_sine_reads_twice_its_amplitude() {
        let mut band = BandPass::new(&band_80(), 500, 4);
        let amplitude = 1e-5;
        let w = 2.0 * PI * 29.5 / 500.0;
        let out = (0..1500)
            .map(|i| band.push(amplitude * (w * i as f64).sin()))
            .last()
            .unwrap();
        assert!((out / (2.0 * amplitude) - 1.0).abs() < 0.05, "{out}");
    }

    #[test]
    fn silence_reads_zero_and_the_window_follows_the_high_edge() {
        let mut band = BandPass::new(&band_80(), 500, 4);
        assert_eq!(band.window, 6);
        assert_eq!(band.push(0.0), 0.0);
        let low = BandPassDesign {
            low_hz: 1.0,
            high_hz: 3.0,
            order: 128,
        };
        assert_eq!(BandPass::new(&low, 500, 4).window, 83);
    }
}
