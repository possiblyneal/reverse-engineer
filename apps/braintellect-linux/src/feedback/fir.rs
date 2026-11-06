//! Windowed-sinc FIR filters, as the app's `ippsFIRGen*` calls design them, and
//! their streaming application (`0x650ed0` constructor, `0x651490` apply).
//!
//! Only the Hamming window is recovered. The band-pass taps and their
//! normalisation are the formula of the `tmp/s43` replay (`a3.py` `taps`) that
//! matched run 4; the low-pass normalisation (unit gain at DC) is inferred.

use std::collections::VecDeque;
use std::f64::consts::PI;

/// Hamming window sample `n` of `len`.
fn hamming(n: usize, len: usize) -> f64 {
    0.54 - 0.46 * (2.0 * PI * n as f64 / (len - 1) as f64).cos()
}

/// Ideal band-limited impulse response between normalised edges `f1 < f2`
/// (cycles per sample), at offset `m` from the filter centre.
fn ideal_bandpass(f1: f64, f2: f64, m: f64) -> f64 {
    if m.abs() < 1e-12 {
        2.0 * (f2 - f1)
    } else {
        ((2.0 * PI * f2 * m).sin() - (2.0 * PI * f1 * m).sin()) / (PI * m)
    }
}

/// `taps` Hamming-windowed band-pass taps for normalised edges `f1`, `f2`
/// (edge frequency over sample rate), scaled to unit gain at the band centre.
pub fn bandpass_taps(f1: f64, f2: f64, taps: usize) -> Vec<f64> {
    let centre = (taps - 1) as f64 / 2.0;
    let h: Vec<f64> = (0..taps)
        .map(|n| ideal_bandpass(f1, f2, n as f64 - centre) * hamming(n, taps))
        .collect();
    let gain = response(&h, (f1 + f2) / 2.0);
    h.into_iter().map(|v| v / gain).collect()
}

/// `taps` Hamming-windowed low-pass taps for the normalised cutoff `f`, scaled
/// to unit gain at DC.
pub fn lowpass_taps(f: f64, taps: usize) -> Vec<f64> {
    let centre = (taps - 1) as f64 / 2.0;
    let h: Vec<f64> = (0..taps)
        .map(|n| {
            let m = n as f64 - centre;
            let ideal = if m.abs() < 1e-12 {
                2.0 * f
            } else {
                (2.0 * PI * f * m).sin() / (PI * m)
            };
            ideal * hamming(n, taps)
        })
        .collect();
    let sum: f64 = h.iter().sum();
    h.into_iter().map(|v| v / sum).collect()
}

/// Magnitude of the frequency response of `taps` at the normalised frequency `f`.
fn response(taps: &[f64], f: f64) -> f64 {
    let (re, im) = taps
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (n, v)| {
            let w = 2.0 * PI * f * n as f64;
            (re + v * w.cos(), im - v * w.sin())
        });
    re.hypot(im)
}

/// A streaming FIR filter with zero initial history.
pub struct Fir {
    taps: Vec<f64>,
    /// Newest sample first.
    history: VecDeque<f64>,
}

impl Fir {
    pub fn new(taps: Vec<f64>) -> Self {
        Self {
            history: VecDeque::with_capacity(taps.len() + 1),
            taps,
        }
    }

    pub fn push(&mut self, x: f64) -> f64 {
        self.history.push_front(x);
        self.history.truncate(self.taps.len());
        self.taps
            .iter()
            .zip(&self.history)
            .map(|(h, x)| h * x)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandpass_passes_the_band_centre_at_unit_gain_and_rejects_far_bands() {
        let taps = bandpass_taps(23.0 / 500.0, 36.0 / 500.0, 128);
        assert_eq!(taps.len(), 128);
        assert!((response(&taps, 29.5 / 500.0) - 1.0).abs() < 1e-12);
        assert!(response(&taps, 2.0 / 500.0) < 0.05);
        assert!(response(&taps, 120.0 / 500.0) < 0.05);
    }

    #[test]
    fn bandpass_taps_are_symmetric() {
        let taps = bandpass_taps(1.0 / 500.0, 3.0 / 500.0, 128);
        for (a, b) in taps.iter().zip(taps.iter().rev()) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn lowpass_has_unit_dc_gain_and_cuts_above_the_cutoff() {
        let taps = lowpass_taps(150.0 / 500.0, 24);
        assert!((response(&taps, 0.0) - 1.0).abs() < 1e-12);
        assert!(response(&taps, 240.0 / 500.0) < 0.2);
    }

    #[test]
    fn streaming_fir_is_the_convolution_with_zero_history() {
        let mut fir = Fir::new(vec![1.0, 2.0, 3.0]);
        let out: Vec<f64> = [1.0, 0.0, 0.0, 0.0].iter().map(|&x| fir.push(x)).collect();
        assert_eq!(out, vec![1.0, 2.0, 3.0, 0.0]);
    }
}
