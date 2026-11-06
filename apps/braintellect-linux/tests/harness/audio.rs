//! The audio layer's baseline: what the run-8 exercise sounded, derived from its
//! Trigger and Note decisions alone, and a synthetic bank shaped like bank 1002.

use braintellect_linux::audio::bank::Bank;
use braintellect_linux::audio::player::AudioEvent;

use super::fixtures::{self, Decision};
use super::synthetic_bank;

/// Output rate of the replay: the rate of bank 1002's samples
/// (`audio-feedback.md`, "Wave device and instrument 1002").
pub const RATE_HZ: u32 = 96_000;

/// Frames of one bank-1002 note: 0.8 s at [`RATE_HZ`].
pub const NOTE_FRAMES: usize = 76_800;

/// A bank of MIDI notes 40..=84 shaped like bank 1002 (96 kHz mono, 0.8 s per
/// note) with invented samples: note `n` is a constant `n * 100`.
pub fn bank_1002_shaped() -> Bank {
    let notes: Vec<(u8, Vec<u8>)> = (40u8..=84)
        .map(|n| {
            let pcm = vec![i16::from(n) * 100; NOTE_FRAMES];
            (n, synthetic_bank::wav(RATE_HZ, &pcm))
        })
        .collect();
    Bank::parse(&synthetic_bank::bank(&notes)).expect("synthetic bank")
}

/// The notes the baseline sounded: each Trigger starts the Note decided with it,
/// at its time (`time_ms` after block 0, 96 frames per ms), playing a whole 0.8 s
/// sample. `block` is the first block read at that time and is not compared
/// (see `assertions::audio_events`).
pub fn run8_events() -> Vec<AudioEvent> {
    let blocks = fixtures::run8_blocks();
    let decisions = fixtures::run8_decisions();
    let origin = blocks[0].read_ms;
    let notes: Vec<&Decision> = decisions.iter().filter(|d| d.kind == "Note").collect();
    decisions
        .iter()
        .filter(|d| d.kind == "Trigger")
        .zip(notes)
        .map(|(trigger, note)| {
            assert_eq!(
                trigger.time_ms, note.time_ms,
                "a Note is decided with its Trigger"
            );
            let time_ms = i64::try_from(trigger.time_ms).expect("time");
            AudioEvent {
                block: blocks
                    .iter()
                    .position(|b| b.read_ms == time_ms)
                    .expect("a block read at the decision time"),
                time_ms,
                note: i32::try_from(note.new).expect("note"),
                sample_index: u64::try_from(time_ms - origin).expect("time") * 96,
                len: NOTE_FRAMES,
            }
        })
        .collect()
}
