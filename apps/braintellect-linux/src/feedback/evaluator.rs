//! The SMART-2 `Evaluator` (`style = coherenceChecker`): the control code that
//! `ExprNoteSelection` and `ExprTriggerAll` decode, from the Aggregators' 60 s band
//! averages. Evidence, and what is observed against inferred, in
//! `docs/reverse-engineering/braintellect2/audio-feedback.md`, "SMART-2 Evaluator".
//!
//! Only the Balance path is implemented. The Coherence series (Ch3/Ch4) are not
//! computed, so the Coherence rule, which would win over Balance, never fires here.

use super::{BandAmplitudes, RATE_HZ};

/// `defaultControlValue`.
pub const DEFAULT_CODE: u32 = 111;
/// `averageEpoch`, in seconds.
const EPOCH_S: usize = 60;
/// `valuesToCheck`: how many epoch values a series holds and must all pass.
const VALUES_TO_CHECK: usize = 2;
/// `balanceThreshold`.
const BALANCE_THRESHOLD: f32 = 0.15;
/// The Ch3/Ch4 Balance scales channel B's average by this before comparing; inferred from
/// the logged values (the ratio is exact to the `float32` bit, its origin is not known).
const CH34_B_GAIN: f64 = 1.08;
/// Code of each Balance series before the `+ 1` (latest below 0) or `+ 2` offset: Band
/// 10 gives 11, Band 80 81, plus 100 for Channels 3n4.
const CODE_BASE: [u32; BALANCES] = [11, 81, 111, 181];
/// Balance series: Ch1/Ch2 Band 10, Ch1/Ch2 Band 80, Ch3/Ch4 Band 10, Ch3/Ch4 Band 80.
pub const BALANCES: usize = 4;

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
    series: [Vec<f32>; BALANCES],
    code: u32,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self {
            samples: 0,
            series: std::array::from_fn(|_| Vec::new()),
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

    /// Account for `len` samples, the last of which has Balance values `latest`. When the
    /// block completes an epoch the latest values enter their series and the code is
    /// decided; the new code is in effect for this block.
    pub fn end_block(&mut self, len: usize, latest: [f32; BALANCES]) {
        self.samples += len;
        if !self.samples.is_multiple_of(EPOCH_S * RATE_HZ) {
            return;
        }
        for (series, value) in self.series.iter_mut().zip(latest) {
            if series.len() == VALUES_TO_CHECK {
                series.remove(0);
            }
            series.push(value);
        }
        self.code = self.decide();
    }

    /// Among the full series whose every value passes the threshold, the one with the
    /// largest latest magnitude; the default if none.
    fn decide(&self) -> u32 {
        let mut winner: Option<(f32, u32)> = None;
        for (series, base) in self.series.iter().zip(CODE_BASE) {
            let full = series.len() == VALUES_TO_CHECK;
            if !full || !series.iter().all(|v| v.abs() > BALANCE_THRESHOLD) {
                continue;
            }
            let latest = series[VALUES_TO_CHECK - 1];
            if winner.is_none_or(|(best, _)| latest.abs() > best) {
                winner = Some((latest.abs(), base + if latest < 0.0 { 1 } else { 2 }));
            }
        }
        winner.map_or(DEFAULT_CODE, |(_, code)| code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn code_is_the_default_until_two_epochs_pass() {
        let mut evaluator = Evaluator::default();
        let latest = balances(amplitudes(1.0, 2.0));
        evaluator.end_block(EPOCH_S * RATE_HZ, latest);
        assert_eq!(evaluator.code(), DEFAULT_CODE);
        evaluator.end_block(EPOCH_S * RATE_HZ, latest);
        assert_eq!(evaluator.code(), 13);
    }

    #[test]
    fn a_larger_channel_a_ends_the_code_in_two() {
        let mut evaluator = Evaluator::default();
        let latest = balances(amplitudes(2.0, 1.0));
        evaluator.end_block(EPOCH_S * RATE_HZ, latest);
        evaluator.end_block(EPOCH_S * RATE_HZ, latest);
        assert_eq!(evaluator.code(), 12);
    }
}
