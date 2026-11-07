//! The run-8 SMART-2 exercise replayed once from the decoded EEG through the
//! Source chain and the feedback graph. Shared by the exercise and audio parity
//! tests.
//!
//! The Source chain starts warm: it runs the earlier stream with the interference
//! filter off, and the 45 Hz low-pass is switched in cold at the graph's second
//! sample, which reproduces the app's logged Source output bit for bit.
//!
//! Inputs the clone does not derive are the logged ones: the counter timer's clock reads and
//! the 16-tap-mean sum order of each band-pass call. The Evaluator code is the logged one
//! ([`Codes::Logged`]) or the clone's own ([`Codes::Evaluated`]).

use std::sync::OnceLock;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::feedback::{
    BlockClock, Interference1, Interference2, Mode, Smart2Graph, SourceFilters, BLOCK_LEN, RATE_HZ,
};

use super::fixtures::{self, Run8Block};

/// Consumed-sample index the app's graph started at.
const GRAPH_START: usize = 25600;
/// Consumed samples of the EEG fixture before the graph start.
const PREROLL: usize = 4000;
/// The counter timer started one counter interval (500 ms) before the first
/// logged read.
const TIMER_BEFORE_FIRST_BLOCK_MS: i64 = 500;

/// Where a replay takes the Evaluator code of each block from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codes {
    /// The code the original logged for the block (`Run8Block::control`).
    Logged,
    /// The clone's Evaluator, from the band averages it computes.
    Evaluated,
}

pub struct Replay {
    pub blocks: Vec<Run8Block>,
    pub decisions: Vec<braintellect_linux::feedback::BlockDecision>,
    pub band_index: Vec<usize>,
}

/// The replay, run once and shared by the tests.
pub fn replay() -> &'static Replay {
    static RUN: OnceLock<Replay> = OnceLock::new();
    RUN.get_or_init(build)
}

/// The original-mode replay under the clone's own Evaluator codes, run once and shared by
/// the tests.
pub fn replay_evaluated() -> &'static Replay {
    static RUN: OnceLock<Replay> = OnceLock::new();
    RUN.get_or_init(|| {
        replay_from(
            &fixtures::run8_eeg(),
            GRAPH_START - PREROLL,
            GRAPH_START,
            GRAPH_START + 1,
            fixtures::run8_blocks(),
            Mode::Original,
            Codes::Evaluated,
        )
    })
}

/// The fixed-mode replay (see [`replay_in`]), run once and shared by the tests.
pub fn replay_fixed() -> &'static Replay {
    static RUN: OnceLock<Replay> = OnceLock::new();
    RUN.get_or_init(|| replay_in(Mode::Fixed))
}

/// How far one replay's output departs from another's over the same blocks.
#[derive(Debug, PartialEq, Eq)]
pub struct Deviation {
    /// Band amplitudes (every band, every sample) whose `float32` bits differ.
    pub amplitudes: usize,
    pub amplitudes_total: usize,
    /// Blocks holding at least one differing amplitude.
    pub blocks: usize,
    pub blocks_total: usize,
    /// Blocks whose Trigger decision differs.
    pub triggers: usize,
    /// Blocks whose note at a trigger differs.
    pub notes: usize,
}

/// What `other` changes against `base`: amplitudes, blocks and decisions.
pub fn deviation(base: &Replay, other: &Replay) -> Deviation {
    let mut d = Deviation {
        amplitudes: 0,
        amplitudes_total: 0,
        blocks: 0,
        blocks_total: base.decisions.len(),
        triggers: 0,
        notes: 0,
    };
    for (a, b) in base.decisions.iter().zip(&other.decisions) {
        let before = d.amplitudes;
        for (x, y) in a.samples.iter().zip(&b.samples) {
            for (p, q) in x.bands.iter().zip(&y.bands) {
                d.amplitudes_total += 1;
                d.amplitudes += usize::from(p.to_bits() != q.to_bits());
            }
        }
        d.blocks += usize::from(d.amplitudes > before);
        d.triggers += usize::from(a.trigger != b.trigger);
        d.notes += usize::from(a.note_at_trigger != b.note_at_trigger);
    }
    d
}

/// The replay with every band-pass call's 16-tap-mean flag set to `scalar_head`, in place
/// of the logged ones (the live sum-order policies; `audio-feedback.md`, "Live sum order").
pub fn replay_with_flags(scalar_head: bool) -> Replay {
    let blocks = fixtures::run8_blocks()
        .into_iter()
        .map(|b| Run8Block {
            scalar_heads: [scalar_head; 8],
            ..b
        })
        .collect();
    replay_from(
        &fixtures::run8_eeg(),
        GRAPH_START - PREROLL,
        GRAPH_START,
        GRAPH_START + 1,
        blocks,
        Mode::Original,
        Codes::Logged,
    )
}

/// The replay in `mode` under the logged per-block inputs. [`Mode::Fixed`] ignores the
/// logged sum orders and switches the 45 Hz low-pass in at the first graph sample.
pub fn replay_in(mode: Mode) -> Replay {
    replay_from(
        &fixtures::run8_eeg(),
        GRAPH_START - PREROLL,
        GRAPH_START,
        GRAPH_START + 1,
        fixtures::run8_blocks(),
        mode,
        Codes::Logged,
    )
}

/// The original-mode replay with every 16-tap-mean flag set to `scalar_head` and the
/// 45 Hz low-pass switched in at consumed sample `graph_start + offset`, in place of the
/// app's logged `+ 1`.
pub fn replay_switching_at(offset: usize, scalar_head: bool) -> Replay {
    let blocks = fixtures::run8_blocks()
        .into_iter()
        .map(|b| Run8Block {
            scalar_heads: [scalar_head; 8],
            ..b
        })
        .collect();
    replay_from(
        &fixtures::run8_eeg(),
        GRAPH_START - PREROLL,
        GRAPH_START,
        GRAPH_START + offset,
        blocks,
        Mode::Original,
        Codes::Logged,
    )
}

fn build() -> Replay {
    replay_in(Mode::Original)
}

/// The replay of an exercise whose consumed EEG starts at consumed sample `first`, before
/// `graph_start`, under the app's logged per-block inputs. The 45 Hz low-pass is
/// switched in at consumed sample `interference_switch` in [`Mode::Original`] (the app's
/// thread-timed sample, an input) and at `graph_start` in [`Mode::Fixed`]. `codes` picks
/// the Evaluator code of each block.
pub fn replay_from(
    eeg: &[[i32; 8]],
    first: usize,
    graph_start: usize,
    interference_switch: usize,
    blocks: Vec<Run8Block>,
    mode: Mode,
    codes: Codes,
) -> Replay {
    let mut design = fixtures::smart2_design();
    design.source.interference1 = Interference1::LowPass45;
    design.source.interference2 = Interference2::Hz25;
    design.source.low_pass_hz = 150.0;
    design.source.high_pass_hz = 1.5;
    // Bands A..H (`fixtures::STAGES`), the order of the block table's flags.
    let band_index: Vec<usize> = fixtures::STAGES
        .iter()
        .map(|&(label, channel)| {
            design
                .band_passes
                .iter()
                .position(|p| p.channel == channel && p.label == label)
                .expect("band-pass in design")
        })
        .collect();
    // Raw counts per volt at the baseline calibration (`b2-protocol.md`,
    // "Calibration confirmed"): family 12, channelGain 12, sensorGain 10.
    let counts_per_volt = ChannelCalibration::new(12.0, 10.0)
        .expect("baseline calibration")
        .divisor(DeviceFamily::F12);
    let volts = |s: usize| -> [f64; 4] {
        let frame = &eeg[(s - first) / 2];
        std::array::from_fn(|ch| f64::from(frame[ch + 4 * ((s - first) % 2)]) / counts_per_volt)
    };
    let mut warm = design.source.clone();
    warm.interference1 = Interference1::Off;
    let mut source: [SourceFilters; 4] =
        std::array::from_fn(|_| SourceFilters::new(&warm, RATE_HZ).unwrap());
    let interference_switch = mode.interference_switch(graph_start, interference_switch);
    let mut filtered = |s: usize| -> [f64; 4] {
        if s == interference_switch {
            for chain in &mut source {
                chain.set_interference1(design.source.interference1, RATE_HZ);
            }
        }
        let x = volts(s);
        std::array::from_fn(|ch| source[ch].push(x[ch]))
    };
    for s in first..graph_start {
        filtered(s);
    }
    let mut graph = Smart2Graph::new(
        design.clone(),
        blocks[0].read_ms - TIMER_BEFORE_FIRST_BLOCK_MS,
        mode,
    )
    .expect("design");
    let decisions = blocks
        .iter()
        .enumerate()
        .map(|(b, block)| {
            let mut heads = vec![false; design.band_passes.len()];
            for (flag, &at) in block.scalar_heads.iter().zip(&band_index) {
                heads[at] = *flag;
            }
            graph.set_scalar_heads(&heads);
            let samples: Vec<[f64; 4]> = (0..BLOCK_LEN)
                .map(|n| filtered(graph_start + b * BLOCK_LEN + n))
                .collect();
            let clock = BlockClock {
                read_ms: block.read_ms,
                lap_ms: block.lap_ms.unwrap_or(block.read_ms),
            };
            match codes {
                Codes::Logged => graph.process_filtered_block(clock, block.control, &samples),
                Codes::Evaluated => graph.process_evaluated_block(clock, &samples),
            }
        })
        .collect();
    Replay {
        blocks,
        decisions,
        band_index,
    }
}
