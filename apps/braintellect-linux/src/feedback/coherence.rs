//! The `Coherence` element (process `0x646990`) and its `DSPCoherence` estimator
//! (constructor `0x644ac0`, process `0x645aa0`; `audio-feedback.md`, "Coherence"): each
//! input through the element's FIR band-pass, 16 blocks gathered per estimate, then the
//! Welch magnitude-squared coherence of the last 4 s averaged over the band's FFT bins and
//! smoothed by an 8-value mean that starts full of 1.0. Between estimates the element
//! repeats the last one; before the first it outputs 1.0.

use super::band_pass::BandPassDesign;
use super::fir::{bandpass_taps, Fir};
use super::ipp_fft::{Complex, RealFft512};
use super::Smoother;

/// Blocks gathered per input before an estimate (`+0x1ac`, 16 under `processingStyle`
/// `Relaxed`).
const PACKET_BLOCKS: usize = 16;
/// Seconds of history the estimate spans (constant `4.0` at `0x1895398`, times the rate).
const HISTORY_S: f64 = 4.0;
/// The estimate's smoothing mean (`0x6447b0` builds it as `(8, 1.0, 0)`).
const MEAN_WINDOW: usize = 8;
/// `0.54 - 0.46 cos` (constants `0x18cb358`, `0x18cb360`).
const HAMMING: (f64, f64) = (0.54, 0.46);

/// One `Coherence` element between two channels.
pub struct Coherence {
    filters: [Fir; 2],
    /// Filtered samples gathered for the next estimate, per input.
    packet: [Vec<f32>; 2],
    packet_len: usize,
    /// The value output for each sample until the next estimate.
    held: f32,
    estimator: Estimator,
}

impl Coherence {
    /// `block_len` is the samples per stream block.
    pub fn new(design: &BandPassDesign, rate_hz: usize, block_len: usize) -> Self {
        let rate = rate_hz as f64;
        let taps = bandpass_taps(design.low_hz / rate, design.high_hz / rate, design.order);
        let packet_len = PACKET_BLOCKS * block_len;
        Self {
            filters: [Fir::new(taps.clone()), Fir::new(taps)],
            packet: [
                Vec::with_capacity(packet_len),
                Vec::with_capacity(packet_len),
            ],
            packet_len,
            held: 1.0,
            estimator: Estimator::new(design, rate_hz),
        }
    }

    /// Feed one block of the two inputs (volts) and return the value the element outputs
    /// for each of the block's samples: a block that completes a packet outputs the new
    /// estimate, any other the last one.
    pub fn process_block(&mut self, block: &[[f64; 2]]) -> f32 {
        for sample in block {
            for (k, (filter, packet)) in self.filters.iter_mut().zip(&mut self.packet).enumerate() {
                // The FIR output is held in `float32`, as in the band-pass elements.
                packet.push(filter.push(sample[k]) as f32);
            }
        }
        if self.packet[0].len() >= self.packet_len {
            let [x, y] = &mut self.packet;
            self.held = self.estimator.process(x, y);
            x.clear();
            y.clear();
        }
        self.held
    }
}

/// `DSPCoherence`: Welch's estimate over a history of `history` samples, newest first,
/// cut into `segments` Hamming-windowed segments of `n` samples `hop` apart, each zero
/// padded to IPP's 512-point real FFT.
struct Estimator {
    n: usize,
    fft: RealFft512,
    first_bin: usize,
    bins: usize,
    hop: usize,
    segments: usize,
    /// Per input, newest sample first.
    history: [Vec<f32>; 2],
    window: Vec<f32>,
    mean: Smoother,
}

impl Estimator {
    fn new(design: &BandPassDesign, rate_hz: usize) -> Self {
        // The constructor's arguments are `float32`: the band edges and the rate.
        let (low, high, rate) = (
            f64::from(design.low_hz as f32),
            f64::from(design.high_hz as f32),
            rate_hz as f64,
        );
        let n = rate as usize;
        let fft_len = n.next_power_of_two();
        assert_eq!(fft_len, 512, "the Coherence FFT is IPP's order 9");
        let bin_per_hz = f64::from((fft_len as f64 / rate) as f32);
        // `_ftol` truncates.
        let first_bin = (bin_per_hz * low) as usize;
        let bins = f64::from((bin_per_hz * high) as f32).ceil() as usize - first_bin + 1;
        let history = (rate * HISTORY_S) as usize;
        // `Relaxed`: half-overlapping segments.
        let hop = n / 2;
        let overlap = n - hop;
        Self {
            n,
            fft: RealFft512::new(),
            first_bin,
            bins,
            hop,
            segments: (history - overlap) / (n - overlap),
            history: [vec![0.0; history], vec![0.0; history]],
            window: hamming(n),
            mean: Smoother::prefilled(MEAN_WINDOW, 1.0),
        }
    }

    /// Take a packet of each input, oldest first, and return the smoothed estimate.
    fn process(&mut self, x: &[f32], y: &[f32]) -> f32 {
        for (history, packet) in self.history.iter_mut().zip([x, y]) {
            history.rotate_right(packet.len());
            for (slot, &v) in history.iter_mut().zip(packet.iter().rev()) {
                *slot = v;
            }
        }
        let mut pxx = vec![0.0_f32; self.bins];
        let mut pyy = vec![0.0_f32; self.bins];
        let mut cross = vec![(0.0_f32, 0.0_f32); self.bins];
        for s in 0..self.segments {
            let start = self.hop * s;
            let end = (start + self.n).min(self.history[0].len());
            let [sx, sy] = [0, 1].map(|k| self.spectrum(&self.history[k][start..end]));
            for b in 0..self.bins {
                let ((xr, xi), (yr, yi)) = (sx[b], sy[b]);
                pxx[b] += xr * xr + xi * xi;
                pyy[b] += yr * yr + yi * yi;
                // `X * conj(Y)`.
                let (cr, ci) = (xr * yr - xi * -yi, xr * -yi + xi * yr);
                cross[b].0 += cr;
                cross[b].1 += ci;
            }
        }
        let segments = self.segments as f64;
        let average = |v: f32| (f64::from(v) / segments) as f32;
        let mut sum = 0.0_f32;
        for b in 0..self.bins {
            let (xx, yy) = (average(pxx[b]), average(pyy[b]));
            let (re, im) = (average(cross[b].0), average(cross[b].1));
            let power = (f64::from(yy) * f64::from(xx)) as f32;
            if power != 0.0 {
                let (re, im) = (f64::from(re), f64::from(im));
                let c = ((re * re + im * im) / f64::from(power)) as f32;
                sum += c.min(1.0);
            }
        }
        self.mean.push((f64::from(sum) / self.bins as f64) as f32)
    }

    /// The windowed, zero-padded segment's FFT over the band's bins.
    fn spectrum(&self, segment: &[f32]) -> Vec<Complex> {
        let mut x = [0.0_f32; 512];
        for ((slot, &v), &w) in x.iter_mut().zip(segment).zip(&self.window) {
            *slot = v * w;
        }
        self.fft.forward(&x)[self.first_bin..self.first_bin + self.bins].to_vec()
    }
}

/// The symmetric Hamming window of `n` samples as the IPP generic kernel (`0x67535c`)
/// builds it: the cosine by an angle-addition recurrence from both ends, in double
/// precision, each value rounded to `float32`.
fn hamming(n: usize) -> Vec<f32> {
    let step = (std::f64::consts::TAU / (n - 1) as f64).cos();
    let step_sin = (1.0 - step * step).sqrt();
    let mut window = vec![0.0_f32; n];
    let (mut cos, mut sin) = (1.0_f64, 0.0_f64);
    let half = (n - 1) / 2;
    for k in 0..half {
        let w = (HAMMING.0 - HAMMING.1 * cos) as f32;
        window[k] = w;
        window[n - 1 - k] = w;
        (cos, sin) = (cos * step - sin * step_sin, sin * step + cos * step_sin);
    }
    let w = (HAMMING.0 - HAMMING.1 * cos) as f32;
    window[half] = w;
    window[n - 1 - half] = w;
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band10() -> BandPassDesign {
        BandPassDesign {
            low_hz: 1.0,
            high_hz: 3.0,
            order: 128,
        }
    }

    #[test]
    fn estimator_layout_follows_the_constructor() {
        let low = Estimator::new(&band10(), 500);
        assert_eq!(
            (low.n, low.first_bin, low.bins, low.hop, low.segments),
            (500, 1, 4, 250, 7)
        );
        let high = Estimator::new(
            &BandPassDesign {
                low_hz: 23.0,
                high_hz: 36.0,
                order: 128,
            },
            500,
        );
        assert_eq!((high.first_bin, high.bins), (23, 15));
    }

    #[test]
    fn hamming_window_is_symmetric_with_the_textbook_values() {
        let w = hamming(500);
        assert_eq!(w[0], 0.08_f32);
        for k in 0..500 {
            assert_eq!(w[k], w[499 - k]);
            let textbook = 0.54 - 0.46 * (std::f64::consts::TAU * k as f64 / 499.0).cos();
            assert!((f64::from(w[k]) - textbook).abs() < 1e-6);
        }
    }

    #[test]
    fn output_is_one_until_the_first_packet_then_the_estimate() {
        let mut element = Coherence::new(&band10(), 500, 4);
        let tone = |i: usize| (std::f64::consts::TAU * 2.0 * i as f64 / 500.0).sin() * 1e-5;
        let mut outputs = Vec::new();
        for b in 0..PACKET_BLOCKS {
            let block: Vec<[f64; 2]> = (0..4).map(|n| [tone(4 * b + n); 2]).collect();
            outputs.push(element.process_block(&block));
        }
        assert!(outputs[..PACKET_BLOCKS - 1].iter().all(|&v| v == 1.0));
        // Identical inputs are fully coherent.
        assert!((outputs[PACKET_BLOCKS - 1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn unrelated_inputs_lower_the_estimate() {
        let mut element = Coherence::new(&band10(), 500, 4);
        let mut seed = 1_u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(seed >> 8) / f64::from(1_u32 << 24) - 0.5
        };
        let mut last = 1.0;
        for _ in 0..PACKET_BLOCKS * 40 {
            let block: Vec<[f64; 2]> = (0..4).map(|_| [noise(), noise()]).collect();
            last = element.process_block(&block);
        }
        assert!(last < 0.8, "estimate {last}");
    }
}
