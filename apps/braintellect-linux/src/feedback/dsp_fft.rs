//! `DSPFFT::process` (`0x637940`) for the 512-point `Bartlett` discrete FFT, operation for
//! operation as the original runs it (`sensor-status.md`, "Level bit exactness"): the
//! `float32` window, the IPP order-9 real FFT ([`RealFft512`]), the IPP magnitude
//! ([`magnitude`]), then the scale, so every bin rounds as the original's does.

use super::ipp_fft::RealFft512;
use super::ipp_magnitude::magnitude;

/// Samples per transform: `fftOrder` 9.
pub const BLOCK: usize = 512;
/// Bins the FFT element emits: `0..BLOCK / 2`.
pub const BINS: usize = BLOCK / 2;
/// SSE lanes of the IPP Bartlett kernel (`0x6b53f0`).
const LANES: usize = 4;
/// `+0x54`: 1/128 for the 512-point FFT, times the `fftScalingFactor` of 1.0.
const GAIN: f32 = 1.0 / 128.0;

/// The window, its mean and the FFT tables of one `DSPFFT`.
#[derive(Debug, Clone)]
pub struct DspFft {
    fft: RealFft512,
    window: [f32; BLOCK],
    /// `+0x50`: the sequential `float32` sum of the window over `BLOCK`.
    mean: f32,
}

impl DspFft {
    pub fn new() -> Self {
        let window = bartlett();
        let mean = window.iter().fold(0.0, |sum, &w| sum + w) / BLOCK as f32;
        Self {
            fft: RealFft512::new(),
            window,
            mean,
        }
    }

    /// Bins `0..BINS` of the windowed block: each magnitude times `GAIN`, over the
    /// window's mean, in the x87's 53-bit precision and rounded once to `float32`.
    pub fn process(&self, block: &[f32; BLOCK]) -> [f32; BINS] {
        let windowed: [f32; BLOCK] = std::array::from_fn(|i| block[i] * self.window[i]);
        let bins = self.fft.forward(&windowed);
        std::array::from_fn(|k| {
            let scaled = f64::from(magnitude(bins[k])) * f64::from(GAIN);
            (scaled / f64::from(self.mean)) as f32
        })
    }
}

impl Default for DspFft {
    fn default() -> Self {
        Self::new()
    }
}

/// The IPP Bartlett window (`0x66fea0`, kernel `0x6b53f0`): with `s = f32(2 / (n - 1))`,
/// lane `j` of the front half starts at `f32(j * s)` and adds `f32(4 * s)` per step; the
/// back half mirrors it.
fn bartlett() -> [f32; BLOCK] {
    let step = 2.0 / (BLOCK - 1) as f32;
    let stride = LANES as f32 * step;
    let mut window = [0.0; BLOCK];
    for lane in 0..LANES {
        let mut value = lane as f32 * step;
        for i in (lane..BLOCK / 2).step_by(LANES) {
            window[i] = value;
            window[BLOCK - 1 - i] = value;
            value += stride;
        }
    }
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_rises_to_one_and_mirrors() {
        let window = DspFft::new().window;
        assert_eq!(window[0], 0.0);
        assert!(window[BLOCK / 2 - 1] > 0.99);
        assert!((0..BLOCK).all(|i| window[i] == window[BLOCK - 1 - i]));
    }

    #[test]
    fn the_window_is_the_original_four_lane_float32_ramp() {
        // Values the ticket-59 hook read from the original's window buffer (a no-wearer run).
        let window = bartlett();
        assert_eq!(window[1], 0.003_913_894_3_f32);
        assert_eq!(window[65], 0.254_403_1_f32);
        assert_eq!(window[134], 0.524_461_8_f32);
        assert_eq!(window[255], window[256]);
        assert_eq!(window[511], 0.0);
    }

    #[test]
    fn the_scale_divides_by_the_sequential_float32_window_mean() {
        assert_eq!(f64::from(DspFft::new().mean), 0.499_022_156_000_137_33);
    }

    #[test]
    fn a_constant_block_is_four_times_its_level_in_bin_zero() {
        let spectrum = DspFft::new().process(&[1e-5; BLOCK]);
        assert!((spectrum[0] - 4e-5).abs() < 1e-8, "{}", spectrum[0]);
        assert!(spectrum[64..].iter().all(|&v| v < 1e-7));
    }
}
