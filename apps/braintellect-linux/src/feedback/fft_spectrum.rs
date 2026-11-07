//! The `FFT` element in `discrete` style feeding an absolute `VectorAggregator`
//! (receive `0x6382d0`, `DSPFFT::process` `0x637940`; `bagsd-design.md`, "Discrete FFT"): the
//! Source output as stored, gathered in 512-sample blocks, each under a `float32` Bartlett
//! window, the IPP order-9 real FFT, bins `0..256` as magnitudes scaled by `1/128` over the
//! window's mean. A stop inside a block transforms the zero-padded partial one too. The
//! aggregator's single stored vector is the mean of the block spectra.
//!
//! Reported, not yet exact: against exercise 146's stored `FFT-Normal-Order` the model
//! agrees at low bins to a few percent and its high-bin floor is a third of the original's.

use super::assess::stored_volts;
use super::fft::{bartlett, spectrum_scale};
use super::ipp_fft::RealFft512;

const BLOCK: usize = 512;
const BINS: usize = BLOCK / 2;
/// Volts to microvolts, the stored vector's unit.
const MICROVOLTS: f64 = 1e6;

/// One channel's block-mean spectrum.
pub struct FftSpectrum {
    fft: RealFft512,
    window: [f32; BLOCK],
    /// The `DSPFFT` scale over the `float32` window's mean, applied to each magnitude.
    scale: f64,
    counts_per_volt: f64,
    block: Vec<f32>,
    sum: [f64; BINS],
    blocks: usize,
}

impl FftSpectrum {
    pub fn new(counts_per_volt: f64) -> Self {
        let bartlett = bartlett(BLOCK);
        let window: [f32; BLOCK] = std::array::from_fn(|i| bartlett[i] as f32);
        Self {
            fft: RealFft512::new(),
            scale: spectrum_scale(&window.map(f64::from)),
            window,
            counts_per_volt,
            block: Vec::with_capacity(BLOCK),
            sum: [0.0; BINS],
            blocks: 0,
        }
    }

    /// One Source output sample in volts, stored as the session stores it.
    pub fn push(&mut self, volts: f64) {
        self.block
            .push(stored_volts(volts, self.counts_per_volt) as f32);
        if self.block.len() == BLOCK {
            self.transform();
        }
    }

    /// The mean spectrum in microvolts at the stop, the partial block included; all
    /// zeros when no sample arrived.
    pub fn finish(mut self) -> Vec<f64> {
        if !self.block.is_empty() {
            self.transform();
        }
        if self.blocks == 0 {
            return vec![0.0; BINS];
        }
        self.sum
            .iter()
            .map(|s| s / self.blocks as f64 * MICROVOLTS)
            .collect()
    }

    fn transform(&mut self) {
        let mut x = [0.0f32; BLOCK];
        for ((x, &s), &w) in x.iter_mut().zip(&self.block).zip(&self.window) {
            *x = s * w;
        }
        for (sum, (re, im)) in self.sum.iter_mut().zip(self.fft.forward(&x)) {
            *sum += f64::from(re.hypot(im)) * self.scale;
        }
        self.block.clear();
        self.blocks += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS_PER_VOLT: f64 = 1e9;

    #[test]
    fn a_constant_input_is_four_times_its_level_in_bin_zero_only() {
        let mut fft = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..BLOCK).for_each(|_| fft.push(1e-5));
        let spectrum = fft.finish();
        assert_eq!(spectrum.len(), BINS);
        assert!((spectrum[0] - 40.0).abs() < 1e-3, "{}", spectrum[0]);
        assert!(spectrum[64..].iter().all(|&v| v < 0.1));
    }

    #[test]
    fn a_stop_before_any_sample_is_a_zero_spectrum() {
        assert_eq!(FftSpectrum::new(COUNTS_PER_VOLT).finish(), vec![0.0; BINS]);
    }

    #[test]
    fn a_stop_inside_a_block_adds_the_zero_padded_partial_block() {
        let mut fft = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..BLOCK + BLOCK / 2).for_each(|_| fft.push(1e-5));
        let mean = fft.finish()[0];
        assert!(mean < 40.0 && mean > 20.0, "{mean}");
    }
}
