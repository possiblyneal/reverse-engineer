//! `BandPassFilter` `OutAmplitude` (vtable `0x18bf7fc`, rebuild `0x66dc00`,
//! per-block process `0x66e9f0`; `audio-feedback.md`, "BandPassFilter
//! OutAmplitude"): a FIR band-pass, the sliding maximum of its absolute value over
//! `W` samples, then a 16-tap mean scaled by 2.

use std::collections::VecDeque;

use super::fir::{bandpass_taps, Fir};

/// Taps of the final mean (`0x6507e0`), each `2.0 / 16` (constant `0x18953a8`).
const MEAN_TAPS: usize = 16;
const MEAN_GAIN: f32 = 2.0 / MEAN_TAPS as f32;
/// Outputs of a 4-sample block that `ippsFIR_32f` sums one at a time, in order, when
/// the block's destination is 8 bytes past a 16-byte boundary (the head before the
/// first aligned 4-output group). Every other output is summed by the 4-tap SIMD
/// group order of [`mean_simd`].
const SCALAR_HEAD_OUTPUTS: usize = 2;

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
    /// The last `MEAN_TAPS` sliding maxima, oldest first.
    means: VecDeque<f32>,
    /// Position of the next sample within its stream block.
    block_pos: usize,
    block_len: usize,
    scalar_head: bool,
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
            block_pos: 0,
            block_len,
            scalar_head: false,
        }
    }

    /// Choose how the mean of the coming blocks is summed. The app hands the mean's
    /// FIR a destination buffer from the heap: 8 bytes past a 16-byte boundary
    /// (`true`) its first two outputs sum sequentially, 16-byte aligned (`false`,
    /// the default) all four sum in the SIMD group order. The two differ in the last
    /// `f32` bit for about one window in six, and the app's choice follows heap
    /// addresses, so a replay supplies the baseline's choice per block.
    pub fn set_scalar_head(&mut self, scalar_head: bool) {
        self.scalar_head = scalar_head;
    }

    /// Feed one sample (volts) and return the amplitude.
    pub fn push(&mut self, x: f64) -> f64 {
        self.push_stages(x)[1]
    }

    /// Feed one sample (volts) and return the FIR output and the amplitude. The app
    /// holds the FIR output in float32 (f64 sum of the taps, rounded once), so it is
    /// rounded here and the later stages see the rounded value.
    pub fn push_stages(&mut self, x: f64) -> [f64; 2] {
        let fir = f64::from(self.fir.push(x) as f32);
        let y = fir.abs();
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
        // The maxima are `f32` values (the FIR output is rounded to `f32`).
        self.means
            .push_back(self.maxima.front().expect("just pushed").1 as f32);
        if self.means.len() > MEAN_TAPS {
            self.means.pop_front();
        }
        let mut window = [0.0_f32; MEAN_TAPS];
        let start = MEAN_TAPS - self.means.len();
        for (slot, &v) in window[start..].iter_mut().zip(&self.means) {
            *slot = v;
        }
        let scalar = self.scalar_head && self.block_pos < SCALAR_HEAD_OUTPUTS;
        self.block_pos = (self.block_pos + 1) % self.block_len;
        [fir, f64::from(mean(&window, scalar))]
    }
}

/// The 16-tap mean of `window` (oldest first) as `ippsFIR_32f` computes it (thunk
/// `0x66ffc0`, kernel `0x67a000` through `0x6d4b80`): `f32` additions of
/// `value * (2/16)` (exact), in the order the kernel runs.
fn mean(window: &[f32; MEAN_TAPS], scalar: bool) -> f32 {
    let p = window.map(|v| v * MEAN_GAIN);
    if scalar {
        // Head path `0x6d76c7`: one output at a time, oldest tap first.
        p.iter().fold(0.0, |acc, &v| acc + v)
    } else {
        mean_simd(&p)
    }
}

/// The SIMD path's order, recovered from the kernel by probing which operands
/// cancel before which join: the four taps `0..4` as `(p0 + p1) + (p2 + p3)`, then
/// each later group of four taps `4k..4k + 4` joins as `+ p[4k + 3]`, `+ p[4k + 2]`,
/// `+ (p[4k] + p[4k + 1])`.
fn mean_simd(p: &[f32; MEAN_TAPS]) -> f32 {
    let mut sum = (p[0] + p[1]) + (p[2] + p[3]);
    for group in p[4..].chunks_exact(4) {
        sum = sum + group[3] + group[2] + (group[0] + group[1]);
    }
    sum
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

    /// Windows and sums read from the app's own `ippsFIR_32f` kernel (run under
    /// emulation), where the two summing orders differ in the last bit.
    #[test]
    fn the_mean_sums_in_the_kernels_order() {
        let window = |bits: [u32; 16]| bits.map(f32::from_bits);
        let a = window([
            0x3fd2a417, 0x410c4932, 0x35f9c212, 0x409a0066, 0x40b0c069, 0x40deb28a, 0x4081efbf,
            0x40a9851c, 0x3fb29bfe, 0x3fe26d79, 0x3f460971, 0x3ae7b509, 0x40d2ba74, 0x3fed0bd4,
            0x3da37218, 0x3fa49317,
        ]);
        assert_eq!(mean(&a, false).to_bits(), 0x40cb3cb4);
        assert_eq!(mean(&a, true).to_bits(), 0x40cb3cb5);
        let b = window([
            0x40b0c069, 0x40deb28a, 0x4081efbf, 0x40a9851c, 0x3fb29bfe, 0x3fe26d79, 0x3f460971,
            0x3ae7b509, 0x40d2ba74, 0x3fed0bd4, 0x3da37218, 0x3fa49317, 0x3f91f5c6, 0x3ee88ba3,
            0x3ed438fc, 0x3fc7daf9,
        ]);
        assert_eq!(mean(&b, false).to_bits(), 0x409c9d49);
        assert_eq!(mean(&b, true).to_bits(), 0x409c9d4a);
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
