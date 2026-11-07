//! The feedback graph's two behaviours, chosen once where the graph is built.

/// Whether the graph reproduces the original's defects or corrects them.
///
/// Every parity test runs [`Mode::Original`]; `live::run` runs [`Mode::Fixed`]. The
/// corrected defects (`audio-feedback.md`):
/// - the threshold window removes one entry when a value leaves it, not every equal one;
/// - the 45 Hz interference low-pass is switched in at the first graph sample, not at a
///   thread-timed one;
/// - every band-pass uses the scalar-head 16-tap-mean sum order (the "Live sum order"
///   policy), not the heap-alignment-dependent one;
/// - the note selection reads Evaluator code 112 as channel 3 and 113 as channel 4, not the
///   channel 3 and 4 average the design's 102/103 typo leaves them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Original,
    Fixed,
}

impl Mode {
    /// The consumed-sample index at which the 45 Hz low-pass takes over. The original's
    /// is `observed`, set by thread timing, so a replay supplies it; fixed mode switches
    /// at `graph_start`.
    pub fn interference_switch(self, graph_start: usize, observed: usize) -> usize {
        match self {
            Self::Original => observed,
            Self::Fixed => graph_start,
        }
    }
}
