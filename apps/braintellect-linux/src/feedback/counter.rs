//! `ThresholdCounter`: type `timeHigh`, trigger epoch 500 ms (handler `0x642ed0`).

/// `triggerEpoch` of the design, in milliseconds.
/// The design's `triggerEpoch`: the time that must pass on the timer before a pass fires.
pub const TRIGGER_EPOCH_MS: i64 = 500;

/// The counter timer's two clock reads in one block. The timer reads the clock
/// once to test the epoch and, on a fire, reads it again to restart; the second
/// read can trail the first by a few milliseconds (`audio-feedback.md`, "Run 8").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockClock {
    /// The read that tests the epoch.
    pub read_ms: i64,
    /// The read that restarts the timer on a fire.
    pub lap_ms: i64,
}

impl From<i64> for BlockClock {
    /// Both reads at the same millisecond.
    fn from(ms: i64) -> Self {
        Self {
            read_ms: ms,
            lap_ms: ms,
        }
    }
}

pub struct Counter {
    count: u32,
    timer_start_ms: i64,
}

impl Counter {
    /// The SMART-2 counter, its timer reading `timer_start_ms` at construction.
    pub fn smart2(timer_start_ms: i64) -> Self {
        Self {
            count: 0,
            timer_start_ms,
        }
    }

    /// Process one block of pass flags read at `clock`. Each pass increments the count; a pass in a block
    /// read at least one epoch after the timer started zeroes the count and
    /// fires, and the timer restarts.
    pub fn process_block(&mut self, clock: BlockClock, passes: &[bool]) -> bool {
        let epoch_elapsed = clock.read_ms - self.timer_start_ms >= TRIGGER_EPOCH_MS;
        let mut fired = false;
        for _ in passes.iter().filter(|&&p| p) {
            self.count += 1;
            if epoch_elapsed {
                self.count = 0;
                fired = true;
            }
        }
        if fired {
            self.timer_start_ms = clock.lap_ms;
        }
        fired
    }
}
