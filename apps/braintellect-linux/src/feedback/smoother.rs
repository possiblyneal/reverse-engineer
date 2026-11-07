//! The boxcar mean the graph's elements share (`0x654fe0`, the `averageEpoch`
//! smoothing of `Threshold` and the smoother of the note pipe): the mean of the
//! last `window` values, dividing by the number seen so far while warming up.

use std::collections::VecDeque;

pub struct Smoother {
    window: usize,
    ring: VecDeque<f32>,
    /// Running sum, kept in double precision as the app keeps it.
    sum: f64,
}

impl Smoother {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            ring: VecDeque::with_capacity(window + 1),
            sum: 0.0,
        }
    }

    /// A mean that starts full of `value`, so it always divides by `window` (the app's
    /// constructor `0x654f20` with its count flag clear).
    pub fn prefilled(window: usize, value: f32) -> Self {
        let mut ring = VecDeque::with_capacity(window + 1);
        ring.resize(window, value);
        Self {
            window,
            ring,
            sum: f64::from(value) * window as f64,
        }
    }

    /// Add `x` and return the mean of the window, in single precision.
    pub fn push(&mut self, x: f32) -> f32 {
        self.ring.push_back(x);
        self.sum += f64::from(x);
        if self.ring.len() > self.window {
            let old = self.ring.pop_front().expect("window is not empty");
            self.sum -= f64::from(old);
        }
        (self.sum / self.ring.len() as f64) as f32
    }
}
