//! The Source's IIR filters: the 1.5 Hz first-order high-pass (`setHPFilter`
//! `0x554d60`, kind 1, type 5, order 1), read as Butterworth by prewarped bilinear
//! transform (`audio-feedback.md`, "Source-side software filters", Inferred), and
//! the 25 Hz band-stop ([`Notch`]).

use std::f64::consts::PI;

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

/// One IPP biquad section in transposed direct form II, as the `ippsIIR` kernel at
/// `0x68f350` evaluates it. The tap layout is `b0 b1 b2 a0 a1 a2` and `a0` is 1.
struct Biquad {
    taps: [f64; 6],
    delay: [f64; 2],
}

impl Biquad {
    fn push(&mut self, x: f64) -> f64 {
        let [b0, b1, b2, _, a1, a2] = self.taps;
        let y = b0 * x + self.delay[0];
        self.delay[0] = b1 * x + self.delay[1] - a1 * y;
        self.delay[1] = b2 * x - a2 * y;
        y
    }
}

/// The 25 Hz band-stop (`setInterferenceFilter2` `0x555370`) at 500 Hz: two cascaded
/// biquads whose taps the app designs itself in `0x653460` (`audio-feedback.md`,
/// "Source-side software filters").
pub struct Notch {
    sections: [Biquad; 2],
}

impl Notch {
    /// The taps `0x653460` produces for 23.5 to 25.75 Hz at 500 Hz, from running that
    /// code (Smith complex division, 53-bit x87 precision). The gain is folded into the
    /// first section only. The last-bit variant of the first section's `a2` (`...2514`)
    /// gives the same output over the whole run-8 exercise.
    const TAPS: [[f64; 6]; 2] = [
        [
            0.9802055417010919,
            -1.86748240405372,
            0.9802055417010919,
            1.0,
            -1.8927648988910217,
            0.9808192344572515,
        ],
        [
            1.0,
            -1.9051947011162667,
            1.0,
            1.0,
            -1.879538922464532,
            0.9795922720679167,
        ],
    ];

    pub fn hz25() -> Self {
        Self {
            sections: Self::TAPS.map(|taps| Biquad {
                taps,
                delay: [0.0; 2],
            }),
        }
    }

    pub fn push(&mut self, x: f64) -> f64 {
        self.sections.iter_mut().fold(x, |x, s| s.push(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    /// Steady-state gain at `f_hz`: the RMS of a driven sine's tail, times sqrt(2).
    fn measured_gain(mut push: impl FnMut(f64) -> f64, f_hz: f64) -> f64 {
        let w = 2.0 * PI * f_hz / 500.0;
        let (n, tail) = (20_000, 5000);
        let power: f64 = (0..n)
            .map(|i| push((w * i as f64).sin()))
            .skip(n - tail)
            .map(|y| y * y)
            .sum();
        (2.0 * power / tail as f64).sqrt()
    }

    #[test]
    fn highpass_is_half_power_at_the_cutoff_and_blocks_dc() {
        let gain = |f: f64| {
            let mut hp = Iir::highpass(1.5, 500.0);
            measured_gain(|x| hp.push(x), f)
        };
        let at_cutoff = gain(1.5);
        assert!((at_cutoff - FRAC_1_SQRT_2).abs() < 1e-3, "{at_cutoff}");
        let mut hp = Iir::highpass(1.5, 500.0);
        let settled = (0..5000).map(|_| hp.push(1.0)).last().unwrap();
        assert!(settled.abs() < 1e-6);
        assert!((gain(100.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn notch_is_half_power_at_both_edges_and_deep_at_the_centre() {
        let (lo, hi) = (23.5_f64, 25.75_f64);
        let gain = |f: f64| {
            let mut notch = Notch::hz25();
            measured_gain(|x| notch.push(x), f)
        };
        for f in [lo, hi] {
            assert!(
                (gain(f) - FRAC_1_SQRT_2).abs() < 2e-3,
                "{f} Hz: {}",
                gain(f)
            );
        }
        let centre = gain((lo * hi).sqrt());
        assert!(centre < 1e-3, "{centre}");
        for f in [2.0, 100.0] {
            assert!((gain(f) - 1.0).abs() < 0.01, "{f} Hz: {}", gain(f));
        }
    }
}
