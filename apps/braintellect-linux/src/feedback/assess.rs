//! The stored BAGSD report: every series the original's post-session generator writes per
//! channel. The original does not store the SMART-2 graph's band amplitudes: after the
//! exercise its BAGSD flow design (`BAGSD-4-chnl-wDiscreteFFT-v5.1.0`, run `0x44b700`) reads
//! the session's stored filtered data back and writes the type-9 series from it
//! (`session-storage.md`, "Stored series (#56)", "Stored FFT (#59)"). Per channel and filter:
//! the Source output as stored (whole ADC counts), an order-1 Butterworth filter,
//! `OutAmplitude`, then a `discrete` Aggregator of 5 s. Per channel, the same stored output
//! goes through [`FftSpectrum`].

use std::collections::VecDeque;

use super::band_pass::{SlidingMax, MEAN_TAPS};
use super::fft_spectrum::FftSpectrum;
use super::graph::{BLOCK_LEN, CHANNELS};
use super::iir::Biquad;
use crate::storage::{Point, Series};

/// The name of each channel's stored spectrum series: one point per bin, `x` the bin index.
pub const FFT_NORMAL_ORDER: &str = "FFT-Normal-Order";

/// An order-1 Butterworth element of the design (`filterResponse` `infinite`), edges in Hz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Response {
    LowPass(f64),
    BandPass(f64, f64),
    HighPass(f64),
}

/// The `00-90` series (`dataType="00-90"`), in the record's order: `LowPassFilter`
/// `highFrequency` 1, the `BandPassFilter` edges, `HighPassFilter` `lowFrequency` 62.
pub const BANDS_00_90: [(&str, Response); 11] = [
    ("Band-00", Response::LowPass(1.0)),
    ("Band-10", Response::BandPass(1.0, 3.0)),
    ("Band-20", Response::BandPass(3.0, 5.5)),
    ("Band-30", Response::BandPass(5.5, 7.5)),
    ("Band-40", Response::BandPass(7.5, 10.0)),
    ("Band-50", Response::BandPass(10.0, 12.0)),
    ("Band-60", Response::BandPass(12.0, 15.0)),
    ("Band-70", Response::BandPass(15.0, 23.0)),
    ("Band-80", Response::BandPass(23.0, 36.0)),
    ("Band-90", Response::BandPass(36.0, 48.0)),
    ("Band-VHF", Response::HighPass(62.0)),
];
/// The `LMH` series (`dataType="LMH"`) in their `order`, named per channel pair
/// (`High 1&2`, `High 3&4`).
const BANDS_LMH: [(&str, Response); 3] = [
    ("High", Response::BandPass(15.8, 34.0)),
    ("Mid", Response::BandPass(6.8, 15.8)),
    ("Low", Response::BandPass(1.5, 6.8)),
];

/// Every stored filter series of `channel` (0-based), in storage order: `00-90`, `LMH`,
/// then the `LineNoise` 50 and 60 Hz band-passes. The design gives Channel2's 50 Hz
/// band-pass an upper edge of 51.1 Hz, the others 51 (`BPF 50Hz ch2`).
fn design(channel: usize) -> Vec<(String, Response)> {
    let pair = if channel < 2 { "1&2" } else { "3&4" };
    let upper_50 = if channel == 1 { 51.1 } else { 51.0 };
    BANDS_00_90
        .iter()
        .map(|&(name, response)| (name.to_owned(), response))
        .chain(
            BANDS_LMH
                .iter()
                .map(|&(name, response)| (format!("{name} {pair}"), response)),
        )
        .chain([
            ("50Hz".to_owned(), Response::BandPass(49.0, upper_50)),
            ("60Hz".to_owned(), Response::BandPass(59.0, 61.0)),
        ])
        .collect()
}

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
    /// Per channel, its [`design`] series.
    bands: Vec<Vec<BandSeries>>,
    /// The discrete `FFT` of each channel.
    ffts: Vec<FftSpectrum>,
}

struct BandSeries {
    label: String,
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
        let rate = rate_hz as f64;
        let bands = (0..CHANNELS)
            .map(|channel| {
                design(channel)
                    .into_iter()
                    .map(|(label, response)| {
                        let (filter, edge_hz) = match response {
                            Response::LowPass(high) => (Biquad::low_pass(high, rate), high),
                            Response::BandPass(low, high) => {
                                (Biquad::band_pass(low, high, rate), high)
                            }
                            Response::HighPass(low) => (Biquad::high_pass(low, rate), low),
                        };
                        BandSeries {
                            label,
                            filter,
                            maxima: SlidingMax::for_band(edge_hz, rate_hz, BLOCK_LEN),
                            recent: VecDeque::with_capacity(MEAN_TAPS + 1),
                            epoch: VecDeque::with_capacity(epoch_len + 1),
                            points: Vec::new(),
                        }
                    })
                    .collect()
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
        for (bands, &volts) in self.bands.iter_mut().zip(volts) {
            let stored = stored_volts(volts, self.counts_per_volt);
            for band in bands {
                band.push(stored, self.epoch_len);
                if emit {
                    band.points.push(band.point(x));
                }
            }
        }
    }

    /// The series per channel: the [`design`] series, then [`FFT_NORMAL_ORDER`]. A stream
    /// that stops inside an epoch closes each filter series with one more point: the mean of
    /// the last epoch's worth of amplitudes, at the stop time (the 240 016 ms point of
    /// exercise 146). The spectrum is the mean of the block spectra up to the stop.
    pub fn series(&self) -> Vec<Vec<Series>> {
        let closing = !self.samples.is_multiple_of(self.epoch_len);
        self.bands
            .iter()
            .zip(&self.ffts)
            .map(|(bands, fft)| {
                bands
                    .iter()
                    .map(|band| {
                        let mut points = band.points.clone();
                        if closing {
                            points.push(band.point(self.x_ms()));
                        }
                        Series {
                            name: band.label.clone(),
                            points,
                        }
                    })
                    .chain([Series {
                        name: FFT_NORMAL_ORDER.to_owned(),
                        points: fft
                            .spectrum()
                            .into_iter()
                            .enumerate()
                            .map(|(bin, y)| Point { x: bin as f64, y })
                            .collect(),
                    }])
                    .collect()
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

    fn named<'a>(channel: &'a [Series], name: &str) -> &'a Series {
        channel.iter().find(|s| s.name == name).expect(name)
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
        assert_eq!(
            names,
            [
                "Band-00",
                "Band-10",
                "Band-20",
                "Band-30",
                "Band-40",
                "Band-50",
                "Band-60",
                "Band-70",
                "Band-80",
                "Band-90",
                "Band-VHF",
                "High 1&2",
                "Mid 1&2",
                "Low 1&2",
                "50Hz",
                "60Hz",
                FFT_NORMAL_ORDER
            ]
        );
        assert_eq!(series[2][11].name, "High 3&4");
        let xs: Vec<f64> = series[0][0].points.iter().map(|p| p.x).collect();
        assert_eq!(xs, [5000.0, 10000.0, 10016.0]);
    }

    #[test]
    fn the_report_closes_with_the_256_bin_block_mean_spectrum_of_each_channel() {
        let series = run(5008, 2.0, 1e-5);
        let fft = named(&series[0], FFT_NORMAL_ORDER);
        let bins: Vec<f64> = fft.points.iter().map(|p| p.x).collect();
        assert_eq!(bins, (0..256).map(f64::from).collect::<Vec<_>>());
        // 2 Hz at 500 Hz sampling is bin 2.05: the low bins carry it, the top does not.
        assert!(fft.points[2].y > 10.0 * fft.points[200].y);
        assert_eq!(named(&series[3], FFT_NORMAL_ORDER), fft);
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
        let last = |name| named(&series[0], name).points.last().unwrap().y;
        let band_80 = last("Band-80");
        assert!((band_80 / 20.0 - 1.0).abs() < 0.1, "{band_80}");
        assert!(last("Band-10") < 0.3 * band_80);
        assert!(last("Band-00") < 0.3 * band_80);
    }
}
