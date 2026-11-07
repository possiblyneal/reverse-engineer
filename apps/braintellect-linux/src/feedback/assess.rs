//! The stored BAGSD report: the Band 10 and Band 80 series and the `FFT-Normal-Order`
//! spectrum. The original does not store the SMART-2 graph's band amplitudes: after the
//! exercise its `Session Assess 3.4-v3.3` flow design (BAGSD run `0x44b700`) reads the
//! session's stored filtered data back and writes the type-9 series from it
//! (`session-storage.md`, "Stored series (#56)", "Stored FFT (#59)"). Per channel and band:
//! the Source output as stored (whole ADC counts), an order-1 Butterworth band-pass,
//! `OutAmplitude`, then a `discrete` Aggregator of 5 s. Per channel, the same stored output
//! goes through [`FftSpectrum`].

use std::collections::VecDeque;

use super::band_pass::{SlidingMax, MEAN_TAPS};
use super::fft_spectrum::FftSpectrum;
use super::graph::{BAND_HIGH, BAND_LOW, BLOCK_LEN, CHANNELS};
use super::iir::Biquad;
use crate::storage::{Point, Series};

/// The name of each channel's stored spectrum series: one point per bin, `x` the bin index.
pub const FFT_NORMAL_ORDER: &str = "FFT-Normal-Order";

/// Label, low and high edge (Hz) of the stored band-passes (`Band-10`, `Band-80` of
/// the design; the other nine bands are not stored by the clone).
const BANDS: [(&str, f64, f64); 2] = [(BAND_LOW, 1.0, 3.0), (BAND_HIGH, 23.0, 36.0)];
/// `OutAmplitude`'s final mean: `MEAN_TAPS` sliding maxima, each scaled by `2 / 16`
/// (`0x6507e0`, as in the SMART-2 band-pass).
const MEAN_GAIN: f64 = 2.0 / MEAN_TAPS as f64;
/// `averageEpoch` of the Aggregators, in milliseconds.
const EPOCH_MS: usize = 5000;
/// The series are stored in microvolts.
const VOLTS_TO_MICROVOLTS: f64 = 1e6;

/// The stored series of one session, fed the Source output from the graph start.
pub struct StoredSeries {
    counts_per_volt: f64,
    rate_hz: usize,
    epoch_len: usize,
    samples: usize,
    /// Channel-major: `BANDS.len()` per channel.
    bands: Vec<BandSeries>,
    /// The discrete `FFT` of each channel.
    ffts: Vec<FftSpectrum>,
}

struct BandSeries {
    label: &'static str,
    filter: Biquad,
    maxima: SlidingMax,
    /// The last `MEAN_TAPS` sliding maxima.
    recent: VecDeque<f64>,
    /// The last `epoch_len` amplitudes.
    epoch: VecDeque<f64>,
    points: Vec<Point>,
}

impl StoredSeries {
    /// `counts_per_volt` is the session's calibration divisor
    /// (`ChannelCalibration::divisor`).
    pub fn new(counts_per_volt: f64, rate_hz: usize) -> Self {
        let epoch_len = rate_hz * EPOCH_MS / 1000;
        let bands = (0..CHANNELS)
            .flat_map(|_| BANDS)
            .map(|(label, low_hz, high_hz)| BandSeries {
                label,
                filter: Biquad::band_pass(low_hz, high_hz, rate_hz as f64),
                maxima: SlidingMax::for_band(high_hz, rate_hz, BLOCK_LEN),
                recent: VecDeque::with_capacity(MEAN_TAPS + 1),
                epoch: VecDeque::with_capacity(epoch_len + 1),
                points: Vec::new(),
            })
            .collect();
        Self {
            counts_per_volt,
            rate_hz,
            epoch_len,
            samples: 0,
            bands,
            ffts: (0..CHANNELS)
                .map(|_| FftSpectrum::new(counts_per_volt))
                .collect(),
        }
    }

    /// Feed one sample of Source output per channel, in volts.
    pub fn push(&mut self, volts: &[f64; CHANNELS]) {
        self.samples += 1;
        let emit = self.samples.is_multiple_of(self.epoch_len);
        let x = self.x_ms();
        for (fft, &volts) in self.ffts.iter_mut().zip(volts) {
            fft.push(volts);
        }
        for (k, band) in self.bands.iter_mut().enumerate() {
            let stored = stored_volts(volts[k / BANDS.len()], self.counts_per_volt);
            band.push(stored, self.epoch_len);
            if emit {
                band.points.push(band.point(x));
            }
        }
    }

    /// The series per channel: the bands in `BANDS` order, then [`FFT_NORMAL_ORDER`]. A
    /// stream that stops inside an epoch closes each band with one more point: the mean of
    /// the last epoch's worth of amplitudes, at the stop time (the 240 016 ms point of
    /// exercise 146). The spectrum is the mean of the block spectra up to the stop.
    pub fn series(&self) -> Vec<Vec<Series>> {
        let closing = !self.samples.is_multiple_of(self.epoch_len);
        let mut series = self.bands.iter().map(|band| {
            let mut points = band.points.clone();
            if closing {
                points.push(band.point(self.x_ms()));
            }
            Series {
                name: band.label.to_owned(),
                points,
            }
        });
        self.ffts
            .iter()
            .map(|fft| {
                let mut channel: Vec<Series> = series.by_ref().take(BANDS.len()).collect();
                channel.push(Series {
                    name: FFT_NORMAL_ORDER.to_owned(),
                    points: fft
                        .spectrum()
                        .into_iter()
                        .enumerate()
                        .map(|(bin, y)| Point { x: bin as f64, y })
                        .collect(),
                });
                channel
            })
            .collect()
    }

    fn x_ms(&self) -> f64 {
        (self.samples * 1000) as f64 / self.rate_hz as f64
    }
}

impl BandSeries {
    fn push(&mut self, x: f64, epoch_len: usize) {
        let peak = self.maxima.push(self.filter.push(x).abs());
        self.recent.push_back(peak);
        if self.recent.len() > MEAN_TAPS {
            self.recent.pop_front();
        }
        self.epoch
            .push_back(self.recent.iter().sum::<f64>() * MEAN_GAIN);
        if self.epoch.len() > epoch_len {
            self.epoch.pop_front();
        }
    }

    fn point(&self, x: f64) -> Point {
        let mean = self.epoch.iter().sum::<f64>() / self.epoch.len() as f64;
        Point {
            x,
            y: mean * VOLTS_TO_MICROVOLTS,
        }
    }
}

/// The Source output as the session stores it (`dataStorageStyle` `filtered`) and BAGSD
/// reads it back: truncated toward zero to whole ADC counts, then volts as `f32`. Bit
/// exact on every BAGSD band-pass input the #56 capture logged.
pub(super) fn stored_volts(volts: f64, counts_per_volt: f64) -> f64 {
    f64::from(((volts * counts_per_volt).trunc() / counts_per_volt) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const COUNTS_PER_VOLT: f64 = 1e9;

    fn run(samples: usize, f_hz: f64, amplitude: f64) -> Vec<Vec<Series>> {
        let mut stored = StoredSeries::new(COUNTS_PER_VOLT, 500);
        let w = 2.0 * PI * f_hz / 500.0;
        for i in 0..samples {
            stored.push(&[amplitude * (w * i as f64).sin(); CHANNELS]);
        }
        stored.series()
    }

    #[test]
    fn stored_volts_truncate_toward_zero_to_whole_counts() {
        assert_eq!(stored_volts(2.9e-9, COUNTS_PER_VOLT), f64::from(2e-9_f32));
        assert_eq!(stored_volts(-2.9e-9, COUNTS_PER_VOLT), f64::from(-2e-9_f32));
    }

    #[test]
    fn one_point_per_epoch_and_a_closing_point_at_the_stop() {
        let series = run(5008, 2.0, 1e-5);
        assert_eq!(series.len(), CHANNELS);
        let names: Vec<&str> = series[0].iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [BAND_LOW, BAND_HIGH, FFT_NORMAL_ORDER]);
        let xs: Vec<f64> = series[0][0].points.iter().map(|p| p.x).collect();
        assert_eq!(xs, [5000.0, 10000.0, 10016.0]);
    }

    #[test]
    fn the_report_closes_with_the_256_bin_block_mean_spectrum_of_each_channel() {
        let series = run(5008, 2.0, 1e-5);
        let fft = &series[0][2];
        let bins: Vec<f64> = fft.points.iter().map(|p| p.x).collect();
        assert_eq!(bins, (0..256).map(f64::from).collect::<Vec<_>>());
        // 2 Hz at 500 Hz sampling is bin 2.05: the low bins carry it, the top does not.
        assert!(fft.points[2].y > 10.0 * fft.points[200].y);
        assert_eq!(series[3][2], *fft);
    }

    #[test]
    fn a_stop_on_the_epoch_boundary_adds_no_closing_band_point() {
        let whole: Vec<f64> = run(5000, 2.0, 1e-5)[0][0]
            .points
            .iter()
            .map(|p| p.x)
            .collect();
        assert_eq!(whole, [5000.0, 10000.0]);
    }

    #[test]
    fn a_sine_inside_the_band_reads_about_twice_its_amplitude_in_microvolts() {
        let series = run(10_000, 29.5, 1e-5);
        let band_80 = series[0][1].points.last().unwrap().y;
        assert!((band_80 / 20.0 - 1.0).abs() < 0.1, "{band_80}");
        let band_10 = series[0][0].points.last().unwrap().y;
        assert!(band_10 < 0.3 * band_80, "{band_10}");
    }
}
