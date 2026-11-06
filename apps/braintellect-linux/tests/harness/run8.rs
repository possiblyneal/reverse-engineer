//! The run-8 SMART-2 exercise replayed once from the decoded EEG through the
//! Source chain and the feedback graph. Shared by the exercise and audio parity
//! tests.
//!
//! The Source chain starts warm: it runs the earlier stream with the interference
//! filter off, and the 45 Hz low-pass is switched in cold at the graph's second
//! sample, which reproduces the app's logged Source output bit for bit.
//!
//! Inputs the clone does not derive are the logged ones: the Evaluator code, the
//! counter timer's clock reads and the 16-tap-mean sum order of each band-pass call.

use std::sync::OnceLock;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::feedback::{
    BlockClock, Interference1, Interference2, Smart2Graph, SourceFilters, BLOCK_LEN, RATE_HZ,
};

use super::fixtures::{self, Run8Block};

/// Consumed-sample index the app's graph started at.
const GRAPH_START: usize = 25600;
/// Consumed samples of the EEG fixture before the graph start.
const PREROLL: usize = 4000;
/// The counter timer started one counter interval (500 ms) before the first
/// logged read.
const TIMER_BEFORE_FIRST_BLOCK_MS: i64 = 500;

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

fn build() -> Replay {
    replay_from(
        &fixtures::run8_eeg(),
        GRAPH_START - PREROLL,
        GRAPH_START,
        GRAPH_START + 1,
        fixtures::run8_blocks(),
    )
}

/// The replay of an exercise whose consumed EEG starts at consumed sample `first`, before
/// `graph_start`, under the app's logged per-block inputs. The 45 Hz low-pass is
/// switched in at consumed sample `graph_start + 1`.
pub fn replay_from(
    eeg: &[[i32; 8]],
    first: usize,
    graph_start: usize,
    interference_switch: usize,
    blocks: Vec<Run8Block>,
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
            graph.process_filtered_block(clock, block.control, &samples)
        })
        .collect();
    Replay {
        blocks,
        decisions,
        band_index,
    }
}
