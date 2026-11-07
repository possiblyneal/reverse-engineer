//! The FFT pieces the clone's own spectra share: a radix-2 FFT (`PeakFrequency`, the
//! sensor status) and the `DSPFFT::process` (`0x637940`) Bartlett window and magnitude
//! scale (the sensor status, [`super::FftSpectrum`]). The IPP order-9 real FFT the
//! Coherence and the discrete FFT run is [`super::ipp_fft`], kept apart for its exact
//! sum order.

use std::f64::consts::PI;

/// `DSPFFT::process` scales each magnitude by `+0x54 / +0x50`: 1/128 (`+0x54`, as
/// captured for the 512-point FFT) over the window's mean (`+0x50`).
pub(super) const SPECTRUM_GAIN: f64 = 1.0 / 128.0;

/// In-place radix-2 FFT of a power-of-two length.
pub fn radix2(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((step * k as f64).cos(), (step * k as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                (re[a], im[a]) = (re[a] + tr, im[a] + ti);
            }
        }
        len <<= 1;
    }
}

/// The `DSPFFT` Bartlett window of `n` points.
#[must_use]
pub fn bartlett(n: usize) -> Vec<f64> {
    let last = (n - 1) as f64;
    (0..n)
        .map(|i| 1.0 - (2.0 * i as f64 / last - 1.0).abs())
        .collect()
}

/// The magnitude scale under `window`: `SPECTRUM_GAIN` over the window's mean.
#[must_use]
pub fn spectrum_scale(window: &[f64]) -> f64 {
    SPECTRUM_GAIN / (window.iter().sum::<f64>() / window.len() as f64)
}
