//! The `FFT` element in `discrete` style feeding an absolute `VectorAggregator`
//! (receive `0x6382d0`, `DSPFFT::process` `0x637940`; `bagsd-design.md`, "Discrete FFT"): the
//! Source output as stored, gathered until the element's trigger count of 500 samples, zero
//! padded to the 512-point transform, each under a `float32` Bartlett window, the IPP order-9
//! real FFT, bins `0..256` as magnitudes scaled by `1/128` over the window's mean. A stop
//! inside a block transforms the zero-padded partial one too. The aggregator's single stored
//! vector is the mean of the block spectra.

use super::assess::stored_volts;
use super::fft::SPECTRUM_GAIN;
use super::ipp_fft::RealFft512;

/// The transform size.
const BLOCK: usize = 512;
/// Samples gathered per transform (the `FFT` element's trigger count); the rest is zero.
const TRIGGER: usize = 500;
const BINS: usize = BLOCK / 2;
/// Volts to microvolts, the stored vector's unit.
const MICROVOLTS: f64 = 1e6;

/// The `float32` Bartlett window as `DSPFFT` builds it: the rising half in four lanes, each
/// lane starting at `i / 255.5` and adding `4 / 255.5` per step, mirrored for the falling half.
fn bartlett() -> [f32; BLOCK] {
    const LANES: usize = 4;
    let slope = 1.0f32 / ((BLOCK - 1) as f32 / 2.0);
    let mut rising = [0.0f32; BLOCK / 2];
    for i in 0..rising.len() {
        rising[i] = if i < LANES {
            i as f32 * slope
        } else {
            rising[i - LANES] + LANES as f32 * slope
        };
    }
    std::array::from_fn(|i| rising[i.min(BLOCK - 1 - i)])
}

/// One channel's block-mean spectrum.
pub struct FftSpectrum {
    fft: RealFft512,
    window: [f32; BLOCK],
    /// The window's mean, summed in `float32` as the original does.
    mean: f64,
    counts_per_volt: f64,
    block: Vec<f32>,
    sum: [f64; BINS],
    blocks: usize,
}

impl FftSpectrum {
    pub fn new(counts_per_volt: f64) -> Self {
        let window = bartlett();
        // The original's mean: a sequential `float32` sum over the window, divided by its length.
        let mean = window.iter().fold(0.0f32, |sum, &w| sum + w) / BLOCK as f32;
        Self {
            fft: RealFft512::new(),
            mean: f64::from(mean),
            window,
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
        for ((x, &s), &w) in x.iter_mut().zip(block).zip(&self.window) {
            *x = s * w;
        }
        let mut out = [0.0; BINS];
        for (out, (re, im)) in out.iter_mut().zip(self.fft.forward(&x)) {
            let magnitude = f64::from((re * re + im * im).sqrt());
            *out = f64::from((magnitude * SPECTRUM_GAIN / self.mean) as f32);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTS_PER_VOLT: f64 = 1e9;

    #[test]
    fn the_window_is_the_original_four_lane_float32_ramp() {
        // Values the ticket-59 hook read from the original's window buffer (a no-wearer run).
        let window = bartlett();
        assert_eq!(window[1], 0.003_913_894_3_f32);
        assert_eq!(window[65], 0.254_403_1_f32);
        assert_eq!(window[134], 0.524_461_8_f32);
        assert_eq!(window[255], window[256]);
        assert_eq!(window[511], 0.0);
        assert!(window.iter().zip(window.iter().rev()).all(|(a, b)| a == b));
    }

    #[test]
    fn the_scale_divides_by_the_sequential_float32_window_mean() {
        let mean = bartlett().iter().fold(0.0f32, |sum, &w| sum + w) / BLOCK as f32;
        assert_eq!(f64::from(mean), 0.499_022_156_000_137_33);
    }

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
