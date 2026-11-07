//! The Optimize-tab sensor status of the `BT-LOD-v5.1.11` design: "Ready",
//! "Adjust Fit" or "Not Available", from the Head2D "Image overlay" evaluator.
//!
//! Evidence and open items: `docs/reverse-engineering/braintellect2/sensor-status.md`.
//! The caller supplies each channel's levels ([`ChannelLevels`], from [`level_db`] on
//! the channel's FFT spectrum); [`SensorStatusEngine`] applies the three fault tests
//! with the `stateChangeDelay` debounce and the engine lifecycle (off while a session
//! plays). Not implemented: the FFT stage and the 50/60 Hz line-noise frequency test.

/// `lineNoiseDifference` of the BT-LOD `Sensors` element, in dB.
pub const LINE_NOISE_DIFFERENCE: f32 = 15.5;
/// `lineNoiseChannelPairDifference`, in dB.
pub const PAIR_DIFFERENCE: f32 = 0.5;
/// `stateChangeDelay`, in milliseconds.
pub const STATE_CHANGE_DELAY_MS: i64 = 1000;
/// The ceiling when no ground item supplies an `errorThreshold` (`0x568d39`).
pub const NO_GROUND_CEILING: f32 = 0.0;
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
            ceiling: NO_GROUND_CEILING,
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
/// (`0x560f40`): the nearest bin, halves up.
#[must_use]
pub fn bin_index(freq_hz: f32, rate_hz: f32, bins: usize) -> usize {
    let width = rate_hz / (2 * bins) as f32;
    (freq_hz / width + 0.5).floor().max(0.0) as usize
}

/// `20 * log10(bin)`; a bin of exactly 0.0 skips the log and keeps `previous`
/// (`0x560f40`).
#[must_use]
pub fn level_db(bin: f32, previous: f32) -> f32 {
    if bin == 0.0 {
        previous
    } else {
        20.0 * bin.log10()
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
