//! `PeakFrequency` (vtable `0x18bc414`, `DSPPkFreq::filter` `0x632d90`;
//! `audio-feedback.md`, "PeakFrequency"): the newest `2^fftOrder` samples under a
//! Hann window, an unscaled real FFT, and the first maximum of the magnitude over
//! the bins of the configured frequency range.

use std::collections::VecDeque;
use std::f64::consts::PI;

/// Constant `4.0` of the amplitude formula (`0x1895398`).
const AMPLITUDE_SCALE: f64 = 4.0;

/// A `PeakFrequency` element's design properties (Hann window only).
#[derive(Debug, Clone, PartialEq)]
pub struct PeakDesign {
    pub fft_order: u32,
    pub low_hz: f64,
    pub high_hz: f64,
}

/// One sample's result.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Peak {
    pub bin: usize,
    pub frequency_hz: f64,
    pub amplitude: f64,
}

pub struct PeakFrequency {
    n: usize,
    low_bin: usize,
    high_bin: usize,
    bin_hz: f64,
    window: Vec<f64>,
    window_mean: f64,
    /// Oldest first, zero until filled.
    ring: VecDeque<f64>,
}

impl PeakFrequency {
    pub fn new(design: &PeakDesign, rate_hz: usize) -> Self {
        let n = 1usize << design.fft_order;
        let bin_hz = rate_hz as f64 / n as f64;
        // Hann denominator N, the variant that matched run 4 (97.8 % of 2000 samples).
        let window: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())
            .collect();
        Self {
            n,
            low_bin: (design.low_hz / bin_hz) as usize,
            high_bin: (design.high_hz / bin_hz) as usize,
            bin_hz,
            window_mean: window.iter().sum::<f64>() / n as f64,
            window,
            ring: VecDeque::from(vec![0.0; n]),
        }
    }

    /// Feed one sample and return the peak of the newest window.
    pub fn push(&mut self, x: f64) -> Peak {
        self.ring.pop_front();
        self.ring.push_back(x);
        let mut re: Vec<f64> = self
            .ring
            .iter()
            .zip(&self.window)
            .map(|(a, w)| a * w)
            .collect();
        let mut im = vec![0.0; self.n];
        fft(&mut re, &mut im);
        let mut best = (self.low_bin, f64::NEG_INFINITY);
        for k in self.low_bin..=self.high_bin {
            let magnitude = re[k].hypot(im[k]);
            if magnitude > best.1 {
                best = (k, magnitude);
            }
        }
        Peak {
            bin: best.0,
            frequency_hz: best.0 as f64 * self.bin_hz,
            amplitude: AMPLITUDE_SCALE / self.n as f64 * best.1 / self.window_mean,
        }
    }
}

/// In-place radix-2 FFT of a power-of-two length.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((step * k as f64).cos(), (step * k as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch1() -> PeakDesign {
        PeakDesign {
            fft_order: 9,
            low_hz: 6.0,
            high_hz: 14.0,
        }
    }

    fn sine_peak(design: &PeakDesign, bin: usize, amplitude: f64) -> Peak {
        let mut pk = PeakFrequency::new(design, 500);
        let w = 2.0 * PI * bin as f64 / 512.0;
        (0..1024)
            .map(|i| pk.push(amplitude * (w * i as f64).sin()))
            .last()
            .unwrap()
    }

    #[test]
    fn a_sine_on_a_bin_reports_that_bin_and_twice_its_amplitude() {
        let peak = sine_peak(&ch1(), 10, 3e-6);
        assert_eq!(peak.bin, 10);
        assert!((peak.frequency_hz - 10.0 * 500.0 / 512.0).abs() < 1e-12);
        // |X| = A * N / 4 under a Hann window; the formula (4 / N) |X| / mean(window)
        // then gives 2 A.
        assert!(
            (peak.amplitude / 6e-6 - 1.0).abs() < 1e-9,
            "{}",
            peak.amplitude
        );
    }

    #[test]
    fn bins_outside_the_range_are_ignored_and_ties_take_the_first() {
        assert_eq!(sine_peak(&ch1(), 3, 1.0).bin, 6);
        assert_eq!(sine_peak(&ch1(), 14, 1.0).bin, 14);
        let ch3 = PeakDesign {
            low_hz: 7.0,
            high_hz: 15.0,
            ..ch1()
        };
        assert_eq!(sine_peak(&ch3, 6, 1.0).bin, 7);
        assert_eq!(sine_peak(&ch3, 15, 1.0).bin, 15);
        let mut pk = PeakFrequency::new(&ch1(), 500);
        assert_eq!(pk.push(0.0).bin, 6);
    }

    #[test]
    fn the_fft_matches_a_direct_dft() {
        let mut re: Vec<f64> = (0..16).map(|i| ((i * 7 % 5) as f64) - 2.0).collect();
        let mut im = vec![0.0; 16];
        let input = re.clone();
        fft(&mut re, &mut im);
        for k in 0..16 {
            let (mut dr, mut di) = (0.0, 0.0);
            for (n, x) in input.iter().enumerate() {
                let w = -2.0 * PI * (k * n) as f64 / 16.0;
                dr += x * w.cos();
                di += x * w.sin();
            }
            assert!((re[k] - dr).abs() < 1e-9 && (im[k] - di).abs() < 1e-9);
        }
    }
}
