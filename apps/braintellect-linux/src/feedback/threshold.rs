//! `Threshold 20`: single, goal `decrease`, auto mode (design `BT-SMART-2-v5.1.2`).
//!
//! Window and pass test are from the static analysis (`0x62e850`, `0x62ed70`);
//! the quantile index is from the run-3 hook log, which the static reading left
//! open: the threshold is the sorted window's element at `trunc(n * lower)`,
//! zero-based, clamped to `n - 1`.

use std::collections::VecDeque;

/// Lower quantile as the app stores it (`0.65` as a single-precision float).
const LOWER: f32 = 0.65;
/// `thresholdEpoch` of the design, in milliseconds.
const THRESHOLD_EPOCH_MS: usize = 5_000;

pub struct Threshold {
    capacity: usize,
    ring: VecDeque<f64>,
    sorted: Vec<f64>,
    threshold: f64,
}

impl Threshold {
    /// The SMART-2 threshold for a stream at `rate_hz`.
    pub fn smart2(rate_hz: usize) -> Self {
        Self {
            capacity: rate_hz * THRESHOLD_EPOCH_MS / 1000,
            ring: VecDeque::new(),
            sorted: Vec::new(),
            threshold: 0.0,
        }
    }

    /// Add one smoothed value to the window and test it against the current
    /// threshold: `decrease` passes when the value is strictly below it.
    pub fn push(&mut self, value: f64) -> bool {
        if self.ring.len() == self.capacity {
            let old = self.ring.pop_front().expect("full ring is not empty");
            let at = self.sorted.partition_point(|x| x.total_cmp(&old).is_lt());
            self.sorted.remove(at);
        }
        self.ring.push_back(value);
        let at = self.sorted.partition_point(|x| x.total_cmp(&value).is_lt());
        self.sorted.insert(at, value);
        value < self.threshold
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
