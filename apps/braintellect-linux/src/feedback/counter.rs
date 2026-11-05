//! `ThresholdCounter`: type `timeHigh`, trigger epoch 500 ms (handler `0x642ed0`).

/// `triggerEpoch` of the design, in milliseconds.
const TRIGGER_EPOCH_MS: i64 = 500;

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

    /// Process one block of pass flags read at clock time `now_ms`. The timer is
    /// read once per block. Each pass increments the count; a pass in a block
    /// read at least one epoch after the timer started zeroes the count and
    /// fires, and the timer restarts.
    pub fn process_block(&mut self, now_ms: i64, passes: &[bool]) -> bool {
        let epoch_elapsed = now_ms - self.timer_start_ms >= TRIGGER_EPOCH_MS;
        let mut fired = false;
        for _ in passes.iter().filter(|&&p| p) {
            self.count += 1;
            if epoch_elapsed {
                self.count = 0;
                fired = true;
            }
        }
        if fired {
            self.timer_start_ms = now_ms;
        }
        fired
    }
}
