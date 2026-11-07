//! `Threshold 20`: single, goal `decrease`, auto mode (design `BT-SMART-2-v5.1.2`).
//!
//! Window and pass test are from the static analysis (`0x62e850`, `0x62ed70`);
//! the quantile index is from the run-3 hook log, which the static reading left
//! open: the threshold is the sorted window's element at `trunc(n * lower)`,
//! zero-based, clamped to `n - 1`. The window is a ring plus a sorted multiset
//! whose erase-by-value drops every equal element (`0x62e850`, `0x62e520`), so
//! `n` is the multiset's size, which can fall below the ring's. [`Mode::Fixed`]
//! erases one entry instead, so the set stays at the ring's size.

use std::collections::VecDeque;

use super::Mode;

/// Lower quantile as the app stores it (`0.65` as a single-precision float).
const LOWER: f32 = 0.65;
/// `thresholdEpoch` of the design, in milliseconds.
const THRESHOLD_EPOCH_MS: usize = 5_000;

pub struct Threshold {
    mode: Mode,
    capacity: usize,
    ring: VecDeque<f64>,
    sorted: Vec<f64>,
    threshold: f64,
}

impl Threshold {
    /// The SMART-2 threshold for a stream at `rate_hz`.
    pub fn smart2(rate_hz: usize, mode: Mode) -> Self {
        Self {
            mode,
            capacity: rate_hz * THRESHOLD_EPOCH_MS / 1000,
            ring: VecDeque::new(),
            sorted: Vec::new(),
            threshold: 0.0,
        }
    }

    /// Add one smoothed value to the window and test it against the current
    /// threshold: `decrease` passes when the value is strictly below it.
    pub fn push(&mut self, value: f64) -> bool {
        let passed = value < self.threshold;
        let old = (self.ring.len() == self.capacity)
            .then(|| self.ring.pop_front().expect("full ring is not empty"));
        self.ring.push_back(value);
        let at = self.sorted.partition_point(|x| x.total_cmp(&value).is_lt());
        self.sorted.insert(at, value);
        if let (Some(old), true) = (old, self.sorted.len() > self.capacity) {
            // `multiset::erase(key)` (`0x62e520`): every element equal to the
            // value leaving the ring goes, so the set can fall short of the ring.
            // Fixed mode removes one of them.
            let from = self.sorted.partition_point(|x| x.total_cmp(&old).is_lt());
            let to = match self.mode {
                Mode::Original => self.sorted.partition_point(|x| x.total_cmp(&old).is_le()),
                Mode::Fixed => from + 1,
            };
            self.sorted.drain(from..to);
        }
        passed
    }

    /// Recompute the threshold from the window, after every input block.
    pub fn end_block(&mut self) {
        let n = self.sorted.len();
        if n == 0 {
            self.threshold = 0.0;
            return;
        }
        let k = ((n as f64 * f64::from(LOWER)) as usize).min(n - 1);
        self.threshold = self.sorted[k];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_value_leaving_the_ring_erases_every_copy_from_the_window() {
        let mut t = Threshold {
            mode: Mode::Original,
            capacity: 3,
            ring: VecDeque::new(),
            sorted: Vec::new(),
            threshold: 0.0,
        };
        for v in [5.0, 1.0, 5.0] {
            t.push(v);
        }
        // The fourth value leaves 5.0 (first in); both copies of 5.0 go.
        t.push(2.0);
        assert_eq!(t.sorted, [1.0, 2.0]);
    }
}
