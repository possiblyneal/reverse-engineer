//! The Source's IIR filters: the 1.5 Hz first-order high-pass (`setHPFilter`
//! `0x554d60`, kind 1, type 5, order 1) and the 25 Hz band-stop
//! (`setInterferenceFilter2` `0x555370`, kind 3, type 5, order 2, 23.5/25.75 Hz).
//!
//! Type 5 is read as Butterworth designed by prewarped bilinear transform
//! (`audio-feedback.md`, "Source-side software filters", Inferred); the IPP
//! generator internals are not recovered.

use std::f64::consts::{PI, SQRT_2};

/// Edges of the `"25 Hz"` interference filter 2 (`0x555370`), in Hz.
pub const NOTCH_25_HZ_EDGES: (f64, f64) = (23.5, 25.75);

/// A direct form I filter, `a[0]` taken as 1.
pub struct Iir {
    b: Vec<f64>,
    a: Vec<f64>,
    x: Vec<f64>,
    y: Vec<f64>,
}

impl Iir {
    fn new(b: Vec<f64>, a: Vec<f64>) -> Self {
        let order = b.len();
        Self {
            b,
            a,
            x: vec![0.0; order],
            y: vec![0.0; order],
        }
    }

    /// First-order Butterworth high-pass at `cutoff_hz`.
    pub fn highpass(cutoff_hz: f64, rate_hz: f64) -> Self {
        let w = (PI * cutoff_hz / rate_hz).tan();
        let b0 = 1.0 / (1.0 + w);
        Self::new(vec![b0, -b0], vec![1.0, (w - 1.0) / (w + 1.0)])
    }

    /// Fourth-order digital band-stop from an order-2 Butterworth analogue
    /// prototype, between `low_hz` and `high_hz`.
    pub fn bandstop(low_hz: f64, high_hz: f64, rate_hz: f64) -> Self {
        let prewarp = |f: f64| (PI * f / rate_hz).tan();
        let (w1, w2) = (prewarp(low_hz), prewarp(high_hz));
        let (w0sq, bw) = (w1 * w2, w2 - w1);
        // s -> BW s / (s^2 + w0^2) on 1 / (s^2 + sqrt(2) s + 1), as polynomials in
        // s (highest power first): numerator (s^2 + w0^2)^2, denominator
        // (s^2 + w0^2)^2 + sqrt(2) BW s (s^2 + w0^2) + BW^2 s^2.
        let num_s = [1.0, 0.0, 2.0 * w0sq, 0.0, w0sq * w0sq];
        let den_s = [
            1.0,
            SQRT_2 * bw,
            2.0 * w0sq + bw * bw,
            SQRT_2 * bw * w0sq,
            w0sq * w0sq,
        ];
        let (num, den) = (bilinear(&num_s), bilinear(&den_s));
        Self::new(
            num.iter().map(|v| v / den[0]).collect(),
            den.iter().map(|v| v / den[0]).collect(),
        )
    }

    pub fn push(&mut self, x: f64) -> f64 {
        self.x.rotate_right(1);
        self.x[0] = x;
        let feed: f64 = self.b.iter().zip(&self.x).map(|(b, x)| b * x).sum();
        let back: f64 = self.a[1..].iter().zip(&self.y).map(|(a, y)| a * y).sum();
        let y = feed - back;
        self.y.rotate_right(1);
        self.y[0] = y;
        y
    }
}

/// A degree-4 polynomial in `s` (highest power first) after `s = (z - 1) / (z + 1)`,
/// scaled by `(z + 1)^4`: `s^k (z + 1)^4 = (z - 1)^k (z + 1)^(4 - k)`.
fn bilinear(p: &[f64; 5]) -> [f64; 5] {
    let mut z = [0.0; 5];
    for (i, coeff) in p.iter().enumerate() {
        let k = 4 - i;
        let mut term = vec![1.0];
        for _ in 0..k {
            term = poly_mul(&term, &[1.0, -1.0]);
        }
        for _ in 0..4 - k {
            term = poly_mul(&term, &[1.0, 1.0]);
        }
        for (zj, t) in z.iter_mut().zip(&term) {
            *zj += coeff * t;
        }
    }
    z
}

fn poly_mul(p: &[f64], q: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; p.len() + q.len() - 1];
    for (i, a) in p.iter().enumerate() {
        for (j, b) in q.iter().enumerate() {
            out[i + j] += a * b;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    /// Steady-state gain at `f_hz`: the RMS of a driven sine's tail, times sqrt(2).
    fn measured_gain(mut filter: Iir, f_hz: f64) -> f64 {
        let w = 2.0 * PI * f_hz / 500.0;
        let (n, tail) = (20_000, 5000);
        let power: f64 = (0..n)
            .map(|i| filter.push((w * i as f64).sin()))
            .skip(n - tail)
            .map(|y| y * y)
            .sum();
        (2.0 * power / tail as f64).sqrt()
    }

    #[test]
    fn highpass_is_half_power_at_the_cutoff_and_blocks_dc() {
        let at_cutoff = measured_gain(Iir::highpass(1.5, 500.0), 1.5);
        assert!((at_cutoff - FRAC_1_SQRT_2).abs() < 1e-3, "{at_cutoff}");
        let mut hp = Iir::highpass(1.5, 500.0);
        let settled = (0..5000).map(|_| hp.push(1.0)).last().unwrap();
        assert!(settled.abs() < 1e-6);
        assert!((measured_gain(Iir::highpass(1.5, 500.0), 100.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn bandstop_is_half_power_at_both_edges_and_deep_at_the_centre() {
        let (lo, hi) = NOTCH_25_HZ_EDGES;
        for f in [lo, hi] {
            let g = measured_gain(Iir::bandstop(lo, hi, 500.0), f);
            assert!((g - FRAC_1_SQRT_2).abs() < 2e-3, "{f} Hz: {g}");
        }
        let centre = measured_gain(Iir::bandstop(lo, hi, 500.0), (lo * hi).sqrt());
        assert!(centre < 1e-3, "{centre}");
        for f in [2.0, 100.0] {
            let g = measured_gain(Iir::bandstop(lo, hi, 500.0), f);
            assert!((g - 1.0).abs() < 0.01, "{f} Hz: {g}");
        }
    }
}
