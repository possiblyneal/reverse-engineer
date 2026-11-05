//! `ExprNoteSelection` and the note pipe with `noteAutoRange` (`audio-feedback.md`,
//! "Run 4 Capture"): the selected PeakFrequency is tracked for its trailing
//! minimum and maximum, smoothed, and mapped linearly onto the sound element's
//! MIDI range. The pipe is `0x59a6d0`, the map `0x578240`.

use std::collections::VecDeque;

use super::Smoother;

/// Trailing samples the min/max tracker spans (30 s at 500 Hz; `0x65a940`).
const TRACKER_WINDOW: usize = 15_000;
/// Samples the note smoother averages (the pipe's smoother window).
const SMOOTHER_WINDOW: usize = 62;
/// The sound element's MIDI range at run time (`0x2d4`/`0x2d8` of `SoundElement`).
const NOTE_LOW: i32 = 40;
const NOTE_HIGH: i32 = 76;
/// `SoundElement`'s default note until one arrives (MIDI 60).
const DEFAULT_NOTE: i32 = 60;

/// The PeakFrequency of channels 1..4 for one sample, in Hz.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakFrequencies(pub [f32; 4]);

/// Sliding minimum and maximum over the trailing `window` values, as monotonic
/// queues of `(index, value)`.
struct Tracker {
    window: usize,
    seen: usize,
    min: VecDeque<(usize, f32)>,
    max: VecDeque<(usize, f32)>,
}

impl Tracker {
    fn new(window: usize) -> Self {
        Self {
            window,
            seen: 0,
            min: VecDeque::new(),
            max: VecDeque::new(),
        }
    }

    fn push(&mut self, x: f32) {
        let i = self.seen;
        self.seen += 1;
        while self.min.back().is_some_and(|&(_, v)| v >= x) {
            self.min.pop_back();
        }
        self.min.push_back((i, x));
        while self.max.back().is_some_and(|&(_, v)| v <= x) {
            self.max.pop_back();
        }
        self.max.push_back((i, x));
        for queue in [&mut self.min, &mut self.max] {
            while queue.front().is_some_and(|&(j, _)| j + self.window <= i) {
                queue.pop_front();
            }
        }
    }

    /// The trailing `(minimum, maximum)`; both are the last value after one push.
    fn range(&self) -> (f32, f32) {
        (
            self.min.front().expect("a value was pushed").1,
            self.max.front().expect("a value was pushed").1,
        )
    }
}

/// The note-map `0x578240`: position of `x` within `[min, max]` scaled onto
/// `[NOTE_LOW, NOTE_HIGH]`, truncated, in single precision. A range that is not
/// positive maps to the low note.
fn map_note(x: f32, min: f32, max: f32) -> i32 {
    let ratio = if max - min > 0.0 {
        (x - min) / (max - min)
    } else {
        0.0
    };
    ((NOTE_HIGH - NOTE_LOW) as f32 * ratio) as i32 + NOTE_LOW
}

/// The SMART-2 note path: one MIDI note per block of PeakFrequency values.
pub struct NoteSelection {
    tracker: Tracker,
    smoother: Smoother,
    latest: i32,
}

impl NoteSelection {
    pub fn smart2() -> Self {
        Self {
            tracker: Tracker::new(TRACKER_WINDOW),
            smoother: Smoother::new(SMOOTHER_WINDOW),
            latest: DEFAULT_NOTE,
        }
    }

    /// The note the previous block produced, or the default before any block:
    /// what the sound element reads when a trigger fires ahead of this block.
    pub fn latest(&self) -> i32 {
        self.latest
    }

    /// Process one block under the Evaluator code `control` and return its note,
    /// taken after the block's last sample.
    pub fn process_block(&mut self, control: u32, block: &[PeakFrequencies]) -> i32 {
        if block.is_empty() {
            return self.latest;
        }
        let mut mean = 0.0;
        for peaks in block {
            let x = select(control, peaks.0);
            self.tracker.push(x);
            mean = self.smoother.push(x);
        }
        let (min, max) = self.tracker.range();
        self.latest = map_note(mean, min, max);
        self.latest
    }
}

/// `ExprNoteSelection`: a single channel or the mean of a pair, by Evaluator code.
/// The design lists 101 where the Evaluator emits 112/113 for the Ch3/Ch4 pair,
/// so those fall to the default, as in the app.
fn select(control: u32, [ch1, ch2, ch3, ch4]: [f32; 4]) -> f32 {
    match control {
        11 | 81 => (ch1 + ch2) / 2.0,
        12 | 82 => ch1,
        13 | 83 => ch2,
        102 | 182 => ch3,
        103 | 183 => ch4,
        _ => (ch3 + ch4) / 2.0,
    }
}
