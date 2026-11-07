//! The SMART-2 `Evaluator` (`style = coherenceChecker`): the control code that
//! `ExprNoteSelection` and `ExprTriggerAll` decode, from the Aggregators' 60 s band
//! averages. Evidence, and what is observed against inferred, in
//! `docs/reverse-engineering/braintellect2/audio-feedback.md`, "SMART-2 Evaluator".
//!
//! The Coherence rule runs first: the first full Coherence series (Ch3/Ch4 Band 10, then
//! Band 80) whose every value passes `coherenceThreshold` takes its code from the Balance
//! series of the same band with the largest latest magnitude. Otherwise the Balance rule
//! decides, and otherwise the default stands.

use super::{BandAmplitudes, RATE_HZ};

/// `defaultControlValue`.
pub const DEFAULT_CODE: u32 = 111;
/// `averageEpoch`, in seconds.
const EPOCH_S: usize = 60;
/// `valuesToCheck`: how many epoch values a series holds and must all pass.
const VALUES_TO_CHECK: usize = 2;
/// `balanceThreshold`.
const BALANCE_THRESHOLD: f32 = 0.15;
/// `coherenceThreshold`.
const COHERENCE_THRESHOLD: f32 = 0.80;
/// The Ch3/Ch4 Balance scales channel B's average by this: the design's expression
/// `((1.08*B)-A)/(A min (B*1.08))` (`Expr Balance Ch3n4 Band 10/80`).
const CH34_B_GAIN: f64 = 1.08;
/// Code of each Balance series before the `+ 1` (latest below 0) or `+ 2` offset: Band
/// 10 gives 11, Band 80 81, plus 100 for Channels 3n4.
const CODE_BASE: [u32; BALANCES] = [11, 81, 111, 181];
/// Balance series: Ch1/Ch2 Band 10, Ch1/Ch2 Band 80, Ch3/Ch4 Band 10, Ch3/Ch4 Band 80.
pub const BALANCES: usize = 4;
/// Coherence series: Ch3/Ch4 Band 10, Ch3/Ch4 Band 80.
pub const COHERENCES: usize = 2;
/// The Balance series on each Coherence series' band.
const SAME_BAND: [[usize; 2]; COHERENCES] = [[0, 2], [1, 3]];

/// The four Balance values, `(B - A) / min(A, B)`, of the Aggregators' `float32` averages
/// (`A` the lower-numbered channel), formed in double precision and rounded to `float32` once.
#[must_use]
pub fn balances(aggregates: BandAmplitudes) -> [f32; BALANCES] {
    let balance = |a: f32, b: f32, b_gain: f64| {
        let (a, b) = (f64::from(a), f64::from(b) * b_gain);
        ((b - a) / a.min(b)) as f32
    };
    [
        balance(aggregates.a, aggregates.b, 1.0),
        balance(aggregates.c, aggregates.d, 1.0),
        balance(aggregates.e, aggregates.f, CH34_B_GAIN),
        balance(aggregates.g, aggregates.h, CH34_B_GAIN),
    ]
}

pub struct Evaluator {
    samples: usize,
    /// Per Balance series, the last `VALUES_TO_CHECK` epoch values, oldest first.
    balances: [Vec<f32>; BALANCES],
    /// Per Coherence series, likewise.
    coherences: [Vec<f32>; COHERENCES],
    code: u32,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self {
            samples: 0,
            balances: std::array::from_fn(|_| Vec::new()),
            coherences: std::array::from_fn(|_| Vec::new()),
            code: DEFAULT_CODE,
        }
    }
}

impl Evaluator {
    /// The current control code.
    #[must_use]
    pub fn code(&self) -> u32 {
        self.code
    }

    /// Account for `len` samples, the last of which has Balance values `balances` and
    /// Coherence values `coherences`. When the block completes an epoch the latest values
    /// enter their series and the code is decided; the new code is in effect for this block.
    pub fn end_block(
        &mut self,
        len: usize,
        balances: [f32; BALANCES],
        coherences: [f32; COHERENCES],
    ) {
        self.samples += len;
        if !self.samples.is_multiple_of(EPOCH_S * RATE_HZ) {
            return;
        }
        let values = balances.into_iter().chain(coherences);
        for (series, value) in self
            .balances
            .iter_mut()
            .chain(&mut self.coherences)
            .zip(values)
        {
            if series.len() == VALUES_TO_CHECK {
                series.remove(0);
            }
            series.push(value);
        }
        self.code = self.decide();
    }

    /// The Coherence rule, else the Balance rule, else the default.
    fn decide(&self) -> u32 {
        let passes = |series: &[f32], threshold: f32| {
            series.len() == VALUES_TO_CHECK && series.iter().all(|v| v.abs() > threshold)
        };
        let candidates: Vec<usize> = match self
            .coherences
            .iter()
            .zip(SAME_BAND)
            .find(|(series, _)| passes(series, COHERENCE_THRESHOLD))
        {
            Some((_, band)) => band.to_vec(),
            None => (0..BALANCES)
                .filter(|&k| passes(&self.balances[k], BALANCE_THRESHOLD))
                .collect(),
        };
        self.largest_latest(&candidates).unwrap_or(DEFAULT_CODE)
    }

    /// The code of the Balance series among `candidates` with the largest latest magnitude
    /// (the first on a tie).
    fn largest_latest(&self, candidates: &[usize]) -> Option<u32> {
        let mut winner: Option<(f32, u32)> = None;
        for &k in candidates {
            let latest = self.balances[k][VALUES_TO_CHECK - 1];
            if winner.is_none_or(|(best, _)| latest.abs() > best) {
                winner = Some((
                    latest.abs(),
                    CODE_BASE[k] + if latest < 0.0 { 1 } else { 2 },
                ));
            }
        }
        winner.map(|(_, code)| code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INCOHERENT: [f32; COHERENCES] = [0.5, 0.5];
    /// Band 80 Ch1/Ch2 has the largest Balance; Band 10 Ch3/Ch4 a small one below
    /// `balanceThreshold`, which the Coherence path does not apply.
    const MIXED: [f32; BALANCES] = [0.05, 3.0, -0.1, 0.0];

    fn amplitudes(a: f32, b: f32) -> BandAmplitudes {
        BandAmplitudes {
            a,
            b,
            c: 1.0,
            d: 1.0,
            e: 1.0,
            f: 1.0,
            g: 1.0,
            h: 1.0,
        }
    }

    fn epochs(values: &[([f32; BALANCES], [f32; COHERENCES])]) -> u32 {
        let mut evaluator = Evaluator::default();
        for &(balances, coherences) in values {
            evaluator.end_block(EPOCH_S * RATE_HZ, balances, coherences);
        }
        evaluator.code()
    }

    #[test]
    fn code_is_the_default_until_two_epochs_pass() {
        let latest = balances(amplitudes(1.0, 2.0));
        assert_eq!(epochs(&[(latest, INCOHERENT)]), DEFAULT_CODE);
        assert_eq!(epochs(&[(latest, INCOHERENT); 2]), 13);
    }

    #[test]
    fn a_larger_channel_a_ends_the_code_in_two() {
        let latest = balances(amplitudes(2.0, 1.0));
        assert_eq!(epochs(&[(latest, INCOHERENT); 2]), 12);
    }

    #[test]
    fn coherence_picks_its_band_over_a_larger_balance_elsewhere() {
        assert_eq!(epochs(&[(MIXED, [0.9, 0.2]); 2]), 112);
    }

    #[test]
    fn coherence_must_pass_in_every_epoch_of_the_series() {
        assert_eq!(epochs(&[(MIXED, [0.7, 0.2]), (MIXED, [0.9, 0.2])]), 83);
    }

    #[test]
    fn band_10_coherence_is_checked_before_band_80() {
        assert_eq!(epochs(&[(MIXED, [0.9, 0.95]); 2]), 112);
    }
}
