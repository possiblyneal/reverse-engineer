//! The deterministic player: turns the feedback graph's trigger decisions into
//! audio events and mixed PCM, with no clock and no device.
//!
//! Recovered behavior (`audio-feedback.md`, "Wave device and instrument 1002"):
//! the SoundElement plays on each trigger the counter delivers, which is at most
//! once per counter interval (500 ms), so the player adds no gate of its own.
//! The play path has no timer and no length cap: a note's whole sample (0.8 s in
//! bank 1002) plays, and a note triggered while another rings mixes with it.
//! The samples play unscaled.

use std::fmt;

use super::bank::{Bank, Sample};

/// A note started by a trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioEvent {
    /// The feedback block whose decision started the note.
    pub block: usize,
    /// The block's time, ms.
    pub time_ms: i64,
    /// MIDI note played.
    pub note: i32,
    /// First output frame of the note, from the player's origin.
    pub sample_index: u64,
    /// Output frames the note lasts.
    pub len: usize,
}

/// Why a trigger could not be played.
#[derive(Debug, PartialEq, Eq)]
pub enum PlayError {
    /// The bank has no sample for the note.
    UnknownNote(i32),
}

impl fmt::Display for PlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayError::UnknownNote(note) => {
                write!(f, "the instrument has no sample for note {note}")
            }
        }
    }
}

impl std::error::Error for PlayError {}

/// Plays a [`Bank`] at one output rate, mono.
pub struct Player<'a> {
    bank: &'a Bank,
    rate_hz: u32,
    origin_ms: i64,
    events: Vec<AudioEvent>,
}

impl<'a> Player<'a> {
    /// A player whose output frame 0 is at `origin_ms`, rendering at `rate_hz`.
    pub fn new(bank: &'a Bank, rate_hz: u32, origin_ms: i64) -> Player<'a> {
        Player {
            bank,
            rate_hz,
            origin_ms,
            events: Vec::new(),
        }
    }

    /// Feed one block's decision: `note_at_trigger` is the note the sound element
    /// reads when the counter fired in this block, `None` when it did not.
    pub fn feed(
        &mut self,
        block: usize,
        time_ms: i64,
        note_at_trigger: Option<i32>,
    ) -> Result<Option<AudioEvent>, PlayError> {
        let Some(note) = note_at_trigger else {
            return Ok(None);
        };
        let sample = self.bank.sample(note).ok_or(PlayError::UnknownNote(note))?;
        let since_origin = u64::try_from(time_ms - self.origin_ms).unwrap_or(0);
        let event = AudioEvent {
            block,
            time_ms,
            note,
            sample_index: since_origin * u64::from(self.rate_hz) / 1000,
            len: self.frames_of(sample),
        };
        self.events.push(event);
        Ok(Some(event))
    }

    /// Every note started so far, in order.
    pub fn events(&self) -> &[AudioEvent] {
        &self.events
    }

    /// `frames` output frames from frame `start`, every ringing note summed and
    /// clipped to 16 bits.
    pub fn mix(&self, start: u64, frames: usize) -> Vec<i16> {
        let mut sum = vec![0i32; frames];
        let end = start + frames as u64;
        for event in &self.events {
            let event_end = event.sample_index + event.len as u64;
            if event.sample_index >= end || event_end <= start {
                continue;
            }
            let sample = self
                .bank
                .sample(event.note)
                .expect("played notes are in the bank");
            for at in event.sample_index.max(start)..event_end.min(end) {
                let frame = usize::try_from(at - event.sample_index).expect("frame");
                sum[usize::try_from(at - start).expect("frame")] += self.frame(sample, frame);
            }
        }
        sum.into_iter()
            .map(|v| {
                i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).expect("clamped")
            })
            .collect()
    }

    fn frames_of(&self, sample: &Sample) -> usize {
        sample.pcm.len() * self.rate_hz as usize / sample.rate_hz as usize
    }

    /// Output frame `n` of `sample`: the nearest source frames, linearly
    /// interpolated, the last held.
    fn frame(&self, sample: &Sample, n: usize) -> i32 {
        let num = n as u64 * u64::from(sample.rate_hz);
        let den = u64::from(self.rate_hz);
        let at = usize::try_from(num / den).expect("frame");
        let a = i64::from(sample.pcm[at]);
        let b = i64::from(*sample.pcm.get(at + 1).unwrap_or(&sample.pcm[at]));
        let frac = i64::try_from(num % den).expect("fraction");
        i32::try_from(a + (b - a) * frac / i64::try_from(den).expect("rate")).expect("16-bit")
    }
}
