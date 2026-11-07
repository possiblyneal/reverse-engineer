//! The `FFT` element in `discrete` style feeding an absolute `VectorAggregator`
//! (receive `0x6382d0`, `DSPFFT::process` `0x637940`; `bagsd-design.md`, "Discrete FFT"): the
//! Source output as stored, gathered until the element's trigger count of 500 samples, zero
//! padded to the 512-point transform, then [`DspFft`]: the `float32` Bartlett window, the IPP
//! order-9 real FFT, the IPP magnitude and the `1/128` over the window's mean. A stop inside a
//! block transforms the zero-padded partial one too. The aggregator's single stored vector is
//! the mean of the block spectra.

use super::assess::stored_volts;
use super::dsp_fft::DspFft;

/// The transform size.
const BLOCK: usize = 512;
/// Samples gathered per transform (the `FFT` element's trigger count); the rest is zero.
const TRIGGER: usize = 500;
const BINS: usize = BLOCK / 2;
/// Volts to microvolts, the stored vector's unit.
const MICROVOLTS: f64 = 1e6;

/// One channel's block-mean spectrum.
pub struct FftSpectrum {
    fft: DspFft,
    counts_per_volt: f64,
    block: Vec<f32>,
    sum: [f64; BINS],
    blocks: usize,
}

impl FftSpectrum {
    pub fn new(counts_per_volt: f64) -> Self {
        Self {
            fft: DspFft::new(),
            counts_per_volt,
            block: Vec::with_capacity(TRIGGER),
            sum: [0.0; BINS],
            blocks: 0,
        }
    }

    /// One Source output sample in volts, stored as the session stores it.
    pub fn push(&mut self, volts: f64) {
        self.block
            .push(stored_volts(volts, self.counts_per_volt) as f32);
        if self.block.len() == TRIGGER {
            let magnitudes = self.magnitudes(&self.block);
            for (sum, magnitude) in self.sum.iter_mut().zip(magnitudes) {
                *sum += magnitude;
            }
            self.blocks += 1;
            self.block.clear();
        }
    }

    /// The mean spectrum in microvolts so far, a partial block included as a stop would
    /// transform it; all zeros when no sample arrived.
    pub fn spectrum(&self) -> Vec<f64> {
        let mut sum = self.sum;
        let mut blocks = self.blocks;
        if !self.block.is_empty() {
            for (sum, magnitude) in sum.iter_mut().zip(self.magnitudes(&self.block)) {
                *sum += magnitude;
            }
            blocks += 1;
        }
        if blocks == 0 {
            return vec![0.0; BINS];
        }
        sum.iter().map(|s| s / blocks as f64 * MICROVOLTS).collect()
    }

    /// One block's `DSPFFT` output: the window, the FFT, `float32` magnitudes, then the x87
    /// `f32(magnitude * (1/128) / mean)`.
    fn magnitudes(&self, block: &[f32]) -> [f64; BINS] {
        let mut x = [0.0f32; BLOCK];
        x[..block.len()].copy_from_slice(block);
        let mut out = [0.0; BINS];
        for (out, bin) in out.iter_mut().zip(self.fft.spectrum(&x)) {
            *out = f64::from(bin);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS_PER_VOLT: f64 = 1e9;

    #[test]
    fn a_constant_input_is_about_four_times_its_level_in_bin_zero_only() {
        let mut fft = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..TRIGGER).for_each(|_| fft.push(1e-5));
        let spectrum = fft.spectrum();
        assert_eq!(spectrum.len(), BINS);
        assert!((spectrum[0] - 40.0).abs() < 0.1, "{}", spectrum[0]);
        assert!(spectrum[64..].iter().all(|&v| v < 0.1));
    }

    #[test]
    fn a_stop_before_any_sample_is_a_zero_spectrum() {
        assert_eq!(
            FftSpectrum::new(COUNTS_PER_VOLT).spectrum(),
            vec![0.0; BINS]
        );
    }

    #[test]
    fn a_stop_inside_a_block_adds_the_zero_padded_partial_block() {
        let mut fft = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..TRIGGER + TRIGGER / 2).for_each(|_| fft.push(1e-5));
        let mean = fft.spectrum()[0];
        assert!(mean < 40.0 && mean > 20.0, "{mean}");
    }

    #[test]
    fn a_block_closes_after_500_samples_not_512() {
        let mut one = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..TRIGGER).for_each(|_| one.push(1e-5));
        let mut two = FftSpectrum::new(COUNTS_PER_VOLT);
        (0..2 * TRIGGER).for_each(|_| two.push(1e-5));
        assert_eq!(one.spectrum(), two.spectrum());
    }
}
