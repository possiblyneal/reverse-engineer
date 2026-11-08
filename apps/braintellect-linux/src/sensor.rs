//! The Optimize-tab sensor status of the `BT-LOD-v5.1.11` design: "Ready",
//! "Adjust Fit" or "Not Available", from the Head2D "Image overlay" evaluator.
//!
//! Evidence and open items: `docs/reverse-engineering/braintellect2/sensor-status.md`.
//! The caller supplies each channel's levels ([`ChannelLevels`], from [`level_db`] on
//! the channel's FFT spectrum); [`SensorStatusEngine`] applies the three fault tests
//! with the `stateChangeDelay` debounce and the engine lifecycle (off while a session
//! plays). [`SensorMonitor`] adds the FFT stage ([`DspFft`]) and level computation in
//! front of it; the spectra, levels, bin choice and the line-noise start-up are held bit
//! exact to a capture of the original; the choice of 50 Hz over 60 Hz was not captured.

use crate::feedback::DspFft;

/// `lineNoiseDifference` of the BT-LOD `Sensors` element, in dB.
pub const LINE_NOISE_DIFFERENCE: f32 = 15.5;
/// `lineNoiseChannelPairDifference`, in dB.
pub const PAIR_DIFFERENCE: f32 = 0.5;
/// `stateChangeDelay`, in milliseconds.
pub const STATE_CHANGE_DELAY_MS: i64 = 1000;
/// The ceiling: the ground item's `errorThreshold` (observed in the 2026-11-08 worn
/// replays; `sensor-status.md`).
pub const GROUND_CEILING_DB: f32 = -10.0;
/// The share of a pair's band levels that must match for the pair to fault
/// (`0x18943f0`, `0x565cb0`).
const PAIR_MATCH_SHARE: f32 = 0.5;
/// The pair test runs only above this difference (`0x1899e6c`, `0x568e8d`).
const PAIR_TEST_FLOOR: f32 = -1.0;

/// What the overlay shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorStatus {
    Ready,
    AdjustFit,
    NotAvailable,
}

impl SensorStatus {
    /// The words of the original's overlay image.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::AdjustFit => "Adjust Fit",
            Self::NotAvailable => "Not Available",
        }
    }
}

/// The `Sensors` element's design constants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorDesign {
    pub line_noise_difference: f32,
    pub pair_difference: f32,
    pub state_change_delay_ms: i64,
    pub ceiling: f32,
}

impl SensorDesign {
    #[must_use]
    pub fn bt_lod() -> Self {
        Self {
            line_noise_difference: LINE_NOISE_DIFFERENCE,
            pair_difference: PAIR_DIFFERENCE,
            state_change_delay_ms: STATE_CHANGE_DELAY_MS,
            ceiling: GROUND_CEILING_DB,
        }
    }
}

/// One channel's levels in dB: the line-noise level (`m`) and the band levels the
/// pair test compares.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelLevels {
    pub line_noise: f32,
    pub bands: Vec<f32>,
}

/// The spectrum bin of `freq_hz` in a spectrum of `bins` values at `rate_hz`
/// (`0x560f40`): the frequency over the bin width, truncated (the rounding term at
/// `0x18943f0` reads as 0.0; captured: 40 Hz is bin 40, 70 Hz is bin 71).
#[must_use]
pub fn bin_index(freq_hz: f32, rate_hz: f32, bins: usize) -> usize {
    let width = rate_hz / (2 * bins) as f32;
    (freq_hz / width).max(0.0) as usize
}

/// A bin of 0.04 reads 0 dB: the captured levels are `20 * log10(bin)` plus
/// 27.9588 dB, which is `20 * log10(1 / 0.04)`.
const LEVEL_REFERENCE: f32 = 0.04;

/// `20 * log10(bin / 0.04)`, each step rounded to `float32` as the original's x87 code
/// does (`0x560f40`; the CRT `log10` is taken in double precision); a bin of exactly 0.0
/// skips the log and keeps `previous`.
#[must_use]
pub fn level_db(bin: f32, previous: f32) -> f32 {
    if bin == 0.0 {
        previous
    } else {
        let ratio = bin / LEVEL_REFERENCE;
        20.0 * (f64::from(ratio).log10() as f32)
    }
}

/// FFT length: `fftOrder` 9.
pub const BLOCK_SAMPLES: usize = crate::feedback::FFT_BLOCK;
/// Source samples per block: 1 s at 500 Hz, discrete (non-overlapping) blocks. The FFT
/// input is these followed by `BLOCK_SAMPLES - SOURCE_SAMPLES` zeros (2026-11-08 worn
/// replays; `sensor-status.md`).
pub const SOURCE_SAMPLES: usize = 500;
/// The display timer's period: the Head2D display re-evaluates the overlay about every
/// 60 ms, not only when a block completes.
pub const TICK_MS: i64 = 60;
/// The Source rate the design runs at.
const RATE_HZ: f32 = 500.0;
/// Spectrum values per block.
const BINS: usize = BLOCK_SAMPLES / 2;
/// Mains frequencies the line-noise test chooses between (`0x1897ab8`, `0x1880338`).
const MAINS_HZ: [f32; 2] = [50.0, 60.0];
/// The band frequencies whose levels the pair test compares (`0x1897aa8`,
/// `0x1897aa0`, `0x1897ab0`). The evaluator vector holds them as 70, 40, 12 Hz
/// (`0x568db5`); the order does not matter to the count.
const PAIR_BAND_HZ: [f32; 3] = [12.0, 40.0, 70.0];
/// Channels of the design.
const CHANNELS: usize = 4;

/// One channel's Head2D level state: the last level of each of the 50, 60, 12,
/// 40 and 70 Hz bins, kept when a bin reads exactly 0.0.
#[derive(Debug, Clone, Default)]
struct BandLevels {
    mains: [f32; 2],
    pair: [f32; 3],
}

impl BandLevels {
    fn update(&mut self, spectrum: &[f32]) {
        let level = |hz: f32, previous: f32| {
            level_db(
                spectrum[bin_index(hz, RATE_HZ, BINS).min(BINS - 1)],
                previous,
            )
        };
        for (l, hz) in self.mains.iter_mut().zip(MAINS_HZ) {
            *l = level(hz, *l);
        }
        for (l, hz) in self.pair.iter_mut().zip(PAIR_BAND_HZ) {
            *l = level(hz, *l);
        }
    }
}

/// The FFT stage and level computation in front of [`SensorStatusEngine`]: Source
/// output in volts, one sample per channel at a time.
///
/// Head2D picks the mains frequency to test once, after 8 channel updates (two
/// blocks), and retries every 8 until it does (`0x565300`); until then the line-noise
/// level is the -120 dB floor (`+0x70`). It takes 60 Hz when the channel-mean 60 Hz
/// level exceeds the 70 Hz control level, else 50 Hz when the 50 Hz mean does. Only the
/// 60 Hz outcome was captured; the 50 Hz branch and the operand order are from the
/// decompile (`sensor-status.md`).
#[derive(Debug, Clone)]
pub struct SensorMonitor {
    engine: SensorStatusEngine,
    fft: DspFft,
    block: [Vec<f32>; CHANNELS],
    levels: [BandLevels; CHANNELS],
    updates: usize,
    line: Option<usize>,
    current: [ChannelLevels; CHANNELS],
}

/// The line-noise level before a mains frequency is chosen (`+0x70`).
const LINE_NOISE_FLOOR_DB: f32 = -120.0;
/// Channel updates between line-noise decisions (`+0x398`).
const LINE_NOISE_TEST_DELAY: usize = 8;

impl SensorMonitor {
    #[must_use]
    pub fn new(design: SensorDesign) -> Self {
        Self {
            engine: SensorStatusEngine::new(design),
            fft: DspFft::new(),
            block: std::array::from_fn(|_| Vec::with_capacity(SOURCE_SAMPLES)),
            levels: std::array::from_fn(|_| BandLevels::default()),
            updates: 0,
            line: None,
            current: std::array::from_fn(|_| ChannelLevels {
                line_noise: LINE_NOISE_FLOOR_DB,
                bands: vec![0.0; PAIR_BAND_HZ.len()],
            }),
        }
    }

    /// The levels after the last completed block.
    #[must_use]
    pub fn levels(&self) -> &[ChannelLevels; CHANNELS] {
        &self.current
    }

    /// Whether the evaluator's three tests fail on the levels after the last block.
    #[must_use]
    pub fn faulted(&self) -> bool {
        self.engine.faulted(&self.current)
    }

    /// The mains frequency choice from the channel means (50 Hz, 60 Hz, 70 Hz control).
    fn choose_line(&self) -> Option<usize> {
        let mean = |level: fn(&BandLevels) -> f32| {
            self.levels.iter().map(level).sum::<f32>() / CHANNELS as f32
        };
        let control = mean(|l| l.pair[2]);
        if mean(|l| l.mains[1]) > control {
            Some(1)
        } else if mean(|l| l.mains[0]) > control {
            Some(0)
        } else {
            None
        }
    }

    /// See [`SensorStatusEngine::set_engine_running`].
    pub fn set_engine_running(&mut self, running: bool) {
        self.engine.set_engine_running(running);
    }

    /// One Source sample per channel (stored as `float32`). Returns whether it completed
    /// a block: after the 500th sample the FFT input (those samples and 12 zeros) updates
    /// the levels. The overlay state changes only in [`Self::tick`].
    pub fn push(&mut self, volts: &[f64; CHANNELS]) -> bool {
        for (block, &v) in self.block.iter_mut().zip(volts) {
            block.push(v as f32);
        }
        if self.block[0].len() < SOURCE_SAMPLES {
            return false;
        }
        for (block, levels) in self.block.iter_mut().zip(&mut self.levels) {
            let mut input = [0.0_f32; BLOCK_SAMPLES];
            input[..SOURCE_SAMPLES].copy_from_slice(block);
            levels.update(&self.fft.process(&input));
            block.clear();
        }
        self.current = std::array::from_fn(|c| ChannelLevels {
            line_noise: self
                .line
                .map_or(LINE_NOISE_FLOOR_DB, |i| self.levels[c].mains[i]),
            bands: self.levels[c].pair.to_vec(),
        });
        self.updates += CHANNELS;
        if self.line.is_none() && self.updates.is_multiple_of(LINE_NOISE_TEST_DELAY) {
            self.line = self.choose_line();
        }
        true
    }

    /// One display timer tick at `now_ms`: the overlay state on the levels after the
    /// last block. Before the first block those are the defaults (all zero), which fail
    /// the pair test.
    pub fn tick(&mut self, now_ms: i64) -> SensorStatus {
        self.engine.update(now_ms, &self.current)
    }
}

/// The Head2D display's decision, fed one display update at a time.
#[derive(Debug, Clone)]
pub struct SensorStatusEngine {
    design: SensorDesign,
    /// `+0x99`, written by `LODEngineRunningSlot`.
    running: bool,
    /// `+0x138` / `+0x13c`: when the current fault run began.
    fault_since_ms: Option<i64>,
}

impl SensorStatusEngine {
    #[must_use]
    pub fn new(design: SensorDesign) -> Self {
        Self {
            design,
            running: false,
            fault_since_ms: None,
        }
    }

    /// `setLODEngineState(state, running)`: the engine runs with the tab shown and
    /// stops when Play is pressed.
    pub fn set_engine_running(&mut self, running: bool) {
        self.running = running;
    }

    /// One display update (`0x569c60`). `channels` are FP1, FP2, T3, T4.
    pub fn update(&mut self, now_ms: i64, channels: &[ChannelLevels; 4]) -> SensorStatus {
        if !self.running {
            return SensorStatus::NotAvailable;
        }
        if self.faulted(channels) {
            let since = *self.fault_since_ms.get_or_insert(now_ms);
            if now_ms - since >= self.design.state_change_delay_ms {
                return SensorStatus::AdjustFit;
            }
        } else {
            self.fault_since_ms = None;
        }
        SensorStatus::Ready
    }

    /// The evaluator `0x568d30`: any of the three tests failing.
    fn faulted(&self, channels: &[ChannelLevels; 4]) -> bool {
        let d = &self.design;
        let min = channels
            .iter()
            .map(|c| c.line_noise)
            .fold(0.0_f32, f32::min);
        let mut fault = channels.iter().any(|c| {
            (min - c.line_noise).abs() > d.line_noise_difference || d.ceiling < c.line_noise
        });
        if d.pair_difference > PAIR_TEST_FLOOR {
            fault |= pair_fault(&channels[0], &channels[1], d.pair_difference);
            fault |= pair_fault(&channels[2], &channels[3], d.pair_difference);
        }
        fault
    }
}

/// `0x565cb0`: the share of band levels that differ by less than `difference`
/// reaches [`PAIR_MATCH_SHARE`].
fn pair_fault(a: &ChannelLevels, b: &ChannelLevels, difference: f32) -> bool {
    let n = a.bands.len().max(b.bands.len());
    if n == 0 {
        return false;
    }
    let level = |c: &ChannelLevels, i: usize| c.bands.get(i).copied().unwrap_or(0.0);
    let matches = (0..n)
        .filter(|&i| (level(a, i) - level(b, i)).abs() < difference)
        .count();
    matches as f32 / n as f32 >= PAIR_MATCH_SHARE
}
