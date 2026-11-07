//! The `float32` real FFT of 512 points the original links from IPP
//! (`ippsFFTInitAlloc_R_32f(order 9, flag 8)`, forward `0x66ff20`, SIMD kernel `0x67831c`;
//! `audio-feedback.md`, "Coherence FFT"), operation for operation so that every output
//! rounds as the original's does.
//!
//! The 512 real samples are read as 256 complex ones (even samples real, odd imaginary)
//! and transformed by a radix-4 decimation-in-time FFT: a twiddle-free 4-point DFT over
//! inputs 64 apart in 6-bit bit-reversed order, then three twiddled stages with quarters
//! of 4, 16 and 64. A split pass then turns the 256 complex bins into the 257 real-input
//! bins.

/// A complex `float32`, `(re, im)`.
pub type Complex = (f32, f32);

const N: usize = 512;
const HALF: usize = N / 2;
/// The quarter length of each twiddled stage.
const STAGES: [usize; 3] = [4, 16, 64];

/// The forward transform with its twiddle and split tables.
#[derive(Debug, Clone)]
pub struct RealFft512 {
    /// Per stage, per index `i` of the quarter: `W^i`, `W^2i`, `W^3i` of the stage length.
    twiddles: [Vec<[Complex; 3]>; 3],
    /// Per bin `k` in `1..=128`: the split pass's two factors.
    split: Vec<(f32, f32)>,
}

impl RealFft512 {
    pub fn new() -> Self {
        Self {
            twiddles: STAGES.map(|quarter| {
                (0..quarter)
                    .map(|i| [1, 2, 3].map(|m| twiddle(m * i, 4 * quarter)))
                    .collect()
            }),
            // `0.5 cos`, and `0.5 - 0.5 sin` with the sine rounded first.
            split: (1..=HALF / 2)
                .map(|k| {
                    let (cos, sin) = unit(k, N);
                    (
                        (0.5 * cos) as f32,
                        (0.5 - 0.5 * f64::from(sin as f32)) as f32,
                    )
                })
                .collect(),
        }
    }

    /// Transform 512 real samples into bins `0..=256` (bins 0 and 256 have no imaginary
    /// part, stored as 0).
    pub fn forward(&self, x: &[f32; N]) -> Vec<Complex> {
        let mut bins = self.complex(x);
        let (re0, im0) = bins[0];
        let nyquist = re0 - im0;
        bins[0] = (re0 + im0, 0.0);
        for k in 1..HALF / 2 {
            let (t0, t1) = self.split[k - 1];
            let ((r, i), (rm, im)) = (bins[k], bins[HALF - k]);
            let (dr, si) = (r - rm, im + i);
            let p = si * t0 + dr * t1;
            let q = t1 * si - t0 * dr;
            bins[k] = (rm + p, q - im);
            bins[HALF - k] = (r - p, q - i);
        }
        bins[HALF / 2].1 = -bins[HALF / 2].1;
        bins.push((nyquist, 0.0));
        bins
    }

    /// The 256-point complex FFT of the samples as `(even, odd)` pairs, natural order out.
    fn complex(&self, x: &[f32; N]) -> Vec<Complex> {
        let z = |n: usize| (x[2 * n], x[2 * n + 1]);
        let quarter = HALF / 4;
        let mut w = vec![(0.0, 0.0); HALF];
        for j in 0..quarter {
            let a = bit_reverse_6(j);
            let y = dft4(z(a), z(a + quarter), z(a + 2 * quarter), z(a + 3 * quarter));
            w[4 * j..4 * j + 4].copy_from_slice(&y);
        }
        for (q, twiddles) in STAGES.into_iter().zip(&self.twiddles) {
            for start in (0..HALF).step_by(4 * q) {
                for (i, t) in twiddles.iter().enumerate() {
                    let p = start + i;
                    // The element a quarter on pairs with the first: it carries `W^2i`.
                    let y = dft4(
                        w[p],
                        mul(w[p + 2 * q], t[0]),
                        mul(w[p + q], t[1]),
                        mul(w[p + 3 * q], t[2]),
                    );
                    for (m, v) in y.into_iter().enumerate() {
                        w[p + m * q] = v;
                    }
                }
            }
        }
        w
    }
}

/// The forward 4-point DFT in the kernel's sum order.
fn dft4(x0: Complex, x1: Complex, x2: Complex, x3: Complex) -> [Complex; 4] {
    let (a, b) = ((x2.0 + x0.0, x2.1 + x0.1), (x0.0 - x2.0, x0.1 - x2.1));
    let (s, d) = ((x3.0 + x1.0, x3.1 + x1.1), (x1.0 - x3.0, x1.1 - x3.1));
    [
        (s.0 + a.0, s.1 + a.1),
        (b.0 + d.1, b.1 - d.0),
        (a.0 - s.0, a.1 - s.1),
        (b.0 - d.1, b.1 + d.0),
    ]
}

fn mul(x: Complex, w: Complex) -> Complex {
    (x.0 * w.0 - x.1 * w.1, x.1 * w.0 + x.0 * w.1)
}

/// `exp(-2 pi i j / n)` rounded to `float32`.
fn twiddle(j: usize, n: usize) -> Complex {
    let (cos, sin) = unit(j, n);
    (cos as f32, -sin as f32)
}

/// `(cos, sin)` of `2 pi j / n`, exact at the quarter turns as in IPP's tables.
fn unit(j: usize, n: usize) -> (f64, f64) {
    let j = j % n;
    if (4 * j).is_multiple_of(n) {
        return [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][4 * j / n];
    }
    let angle = std::f64::consts::TAU * j as f64 / n as f64;
    (angle.cos(), angle.sin())
}

fn bit_reverse_6(j: usize) -> usize {
    j.reverse_bits() >> (usize::BITS - 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector `tmp/s60/mkvec.py` builds: 500 exactly representable values from a
    /// 32-bit LCG, then 12 zeros.
    fn input() -> [f32; N] {
        let mut x = [0.0_f32; N];
        let mut seed = 1_u32;
        for v in x.iter_mut().take(500) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *v = ((seed >> 8) as i32 - (1 << 23)) as f32 * 2.0_f32.powi(-37);
        }
        x
    }

    #[test]
    fn forward_matches_the_original_ipp_kernel_bit_for_bit() {
        // The original's kernel run under Unicorn on `input()`: 514 floats,
        // `[R0, 0, R1, I1, ..., R256, 0]`.
        let expected: Vec<u32> = include_str!("../../tests/fixtures/ipp_real_fft_512.txt")
            .lines()
            .map(|l| u32::from_str_radix(l, 16).unwrap())
            .collect();
        let bins = RealFft512::new().forward(&input());
        let got: Vec<u32> = bins
            .iter()
            .flat_map(|&(re, im)| [re.to_bits(), im.to_bits()])
            .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn a_cosine_lands_in_its_bin() {
        let mut x = [0.0_f32; N];
        for (n, v) in x.iter_mut().enumerate() {
            *v = (std::f64::consts::TAU * 5.0 * n as f64 / N as f64).cos() as f32;
        }
        let bins = RealFft512::new().forward(&x);
        assert!((bins[5].0 - 256.0).abs() < 1e-3);
        assert!(bins
            .iter()
            .enumerate()
            .all(|(k, b)| k == 5 || b.0.abs() < 1e-3));
    }
}
