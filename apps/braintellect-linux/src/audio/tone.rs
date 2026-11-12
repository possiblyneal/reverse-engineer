//! The feedback tones a clinician can choose at session set-up: the original's
//! instrument banks and tones rendered from a SoundFont
//! (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Recorded tones").
//!
//! Neither kind of sample is in Git (`docs/adrs/0004-*.md`, `0006-*.md`). The
//! original banks are copied by [`super::import`]; the rendered tones are made
//! once by [`import_soundfont`] into the same instruments directory, one
//! `tone-<key>.bank` file per tone holding the notes
//! [`NOTE_LOW`]..=[`NOTE_HIGH`] at the sink's rate.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::feedback::{NOTE_HIGH, NOTE_LOW};
use crate::live::OUTPUT_RATE_HZ;

use super::bank::{Bank, BankError, Sample};
use super::player::{PlayError, Player};
use super::soundfont::{render_note, SoundFont, SoundFontError};
use super::{import, load_instrument, InstrumentError};

/// Frames of every rendered note: 0.8 s, the length of the original's notes.
const NOTE_FRAMES: usize = OUTPUT_RATE_HZ as usize * 4 / 5;
/// Starts a rendered tone file.
const MAGIC: &[u8; 5] = b"BTTN1";

/// Where a tone's notes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The original's `dll<id>.BT.dll`.
    Bank(u32),
    /// Rendered from a General MIDI program of a SoundFont.
    Soundfont { program: u16 },
}

/// One selectable feedback tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone {
    /// Stable name; the rendered tone's file is `tone-<key>.bank`.
    pub key: &'static str,
    pub label: &'static str,
    pub source: Source,
}

/// What SMART-2 plays: bank 1002.
pub const DEFAULT_TONE: Tone = Tone {
    key: "strings",
    label: "Acoustic strings (original default)",
    source: Source::Bank(1002),
};

/// Every tone, in the order the set-up screen lists them. The original banks are
/// named by their resource names (`AcousticStrings`, `Piano`, `Celeste`).
pub static TONES: [Tone; 6] = [
    DEFAULT_TONE,
    Tone {
        key: "piano",
        label: "Piano",
        source: Source::Bank(1001),
    },
    Tone {
        key: "celeste",
        label: "Celeste",
        source: Source::Bank(1003),
    },
    Tone {
        key: "harp",
        label: "Harp",
        source: Source::Soundfont { program: 46 },
    },
    Tone {
        key: "guitar",
        label: "Acoustic guitar",
        source: Source::Soundfont { program: 24 },
    },
    Tone {
        key: "xylophone",
        label: "Xylophone",
        source: Source::Soundfont { program: 13 },
    },
];

/// Where [`DEFAULT_TONE`] is in [`TONES`].
#[must_use]
pub fn default_index() -> usize {
    TONES
        .iter()
        .position(|t| t.key == DEFAULT_TONE.key)
        .expect("TONES lists the default tone")
}

/// Whether a tone can be played now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// Not imported yet; the message says how to import it.
    Missing(String),
}

impl Tone {
    /// Where the tone's file lives in the instruments directory `dir`.
    #[must_use]
    pub fn path(&self, dir: &Path) -> PathBuf {
        match self.source {
            Source::Bank(id) => import::bank_path(dir, id),
            Source::Soundfont { .. } => dir.join(format!("tone-{}.bank", self.key)),
        }
    }

    /// Whether the tone's file is in `dir`. A file that exists but cannot be read
    /// is reported when a session loads it.
    #[must_use]
    pub fn availability(&self, dir: &Path) -> Availability {
        if self.path(dir).is_file() {
            return Availability::Ready;
        }
        Availability::Missing(match self.source {
            Source::Bank(_) => format!(
                "{} is not imported; run `braintellect-linux import <original lib dir>`",
                self.label
            ),
            Source::Soundfont { .. } => format!(
                "{} is not imported; run `braintellect-linux import-soundfont <file.sf2>`",
                self.label
            ),
        })
    }
}

/// Load `tone` from the instruments directory `dir`.
///
/// # Errors
/// [`InstrumentError::Missing`] when the tone was never imported,
/// [`InstrumentError::Bank`] when its file is unusable.
pub fn load_tone(dir: &Path, tone: &Tone) -> Result<Bank, InstrumentError> {
    match tone.source {
        Source::Bank(id) => load_instrument(dir, id),
        Source::Soundfont { .. } => {
            let path = tone.path(dir);
            match fs::read(&path) {
                Ok(bytes) => decode(&bytes).map_err(|e| InstrumentError::Bank(path, e)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Err(InstrumentError::Missing(path))
                }
                Err(e) => Err(InstrumentError::Bank(path, BankError::Io(e))),
            }
        }
    }
}

/// The notes a tone preview plays, in order: a rising phrase inside the session's
/// note range.
pub const PREVIEW_NOTES: [i32; 3] = [52, 59, 64];
/// Time between the preview's note starts.
pub const PREVIEW_GAP_MS: i64 = 300;

/// The PCM of a short preview of `bank` at the output rate: [`PREVIEW_NOTES`]
/// started [`PREVIEW_GAP_MS`] apart, each played whole.
///
/// # Errors
/// [`PlayError::UnknownNote`] when the bank lacks a preview note.
pub fn preview(bank: &Bank) -> Result<Vec<i16>, PlayError> {
    let mut player = Player::new(bank, OUTPUT_RATE_HZ, 0);
    let mut end = 0;
    for (i, note) in PREVIEW_NOTES.into_iter().enumerate() {
        let at_ms = PREVIEW_GAP_MS * i64::try_from(i).expect("three notes");
        if let Some(event) = player.feed(i, at_ms, Some(note))? {
            end = end.max(event.sample_index + event.len as u64);
        }
    }
    Ok(player.mix(0, usize::try_from(end).expect("frames")))
}

/// Why a SoundFont could not be imported.
#[derive(Debug)]
pub enum ToneImportError {
    Io(std::io::Error),
    Font(SoundFontError),
    /// No zone of the program plays this note.
    NoteNotCovered {
        program: u16,
        note: i32,
    },
}

impl fmt::Display for ToneImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToneImportError::Io(e) => write!(f, "{e}"),
            ToneImportError::Font(e) => write!(f, "{e}"),
            ToneImportError::NoteNotCovered { program, note } => {
                write!(
                    f,
                    "General MIDI program {program} has no sample for note {note}"
                )
            }
        }
    }
}

impl std::error::Error for ToneImportError {}

impl From<std::io::Error> for ToneImportError {
    fn from(e: std::io::Error) -> Self {
        ToneImportError::Io(e)
    }
}

impl From<SoundFontError> for ToneImportError {
    fn from(e: SoundFontError) -> Self {
        ToneImportError::Font(e)
    }
}

/// Render every SoundFont tone of [`TONES`] from `soundfont` (the bytes of a
/// `.sf2` file) into `dest`, created when new. No tone file is written unless every
/// tone renders and every file writes. Returns the files written.
///
/// # Errors
/// [`ToneImportError::Font`] for an unreadable font or a missing program,
/// [`ToneImportError::NoteNotCovered`] when a program lacks a note of the clamp
/// range, [`ToneImportError::Io`] when a file cannot be written.
pub fn import_soundfont(soundfont: &[u8], dest: &Path) -> Result<Vec<PathBuf>, ToneImportError> {
    let font = SoundFont::parse(soundfont)?;
    let mut rendered = Vec::new();
    for tone in &TONES {
        let Source::Soundfont { program } = tone.source else {
            continue;
        };
        let zones = font.zones(program)?;
        let mut samples = BTreeMap::new();
        for note in NOTE_LOW..=NOTE_HIGH {
            let pcm = render_note(&zones, note, OUTPUT_RATE_HZ, NOTE_FRAMES)
                .ok_or(ToneImportError::NoteNotCovered { program, note })?;
            samples.insert(
                note,
                Sample {
                    rate_hz: OUTPUT_RATE_HZ,
                    pcm,
                },
            );
        }
        rendered.push((tone, Bank::from_samples(samples)));
    }
    fs::create_dir_all(dest)?;
    // Each file is written under a temporary name beside its own, and all are
    // renamed into place only once every one is written.
    let mut written: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (tone, bank) in rendered {
        let path = tone.path(dest);
        let part = path.with_extension("bank.part");
        if let Err(e) = fs::write(&part, encode(&bank)) {
            let _ = fs::remove_file(&part);
            for (part, _) in &written {
                let _ = fs::remove_file(part);
            }
            return Err(e.into());
        }
        written.push((part, path));
    }
    written
        .into_iter()
        .map(|(part, path)| {
            fs::rename(&part, &path)?;
            Ok(path)
        })
        .collect()
}

/// `MAGIC`, then per note `i32 note, u32 rate, u32 frames, frames * i16`, little-endian.
fn encode(bank: &Bank) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for note in bank.notes() {
        let sample = bank.sample(note).expect("a listed note has a sample");
        out.extend(note.to_le_bytes());
        out.extend(sample.rate_hz.to_le_bytes());
        out.extend(
            u32::try_from(sample.pcm.len())
                .expect("frames")
                .to_le_bytes(),
        );
        out.extend(sample.pcm.iter().flat_map(|s| s.to_le_bytes()));
    }
    out
}

fn decode(bytes: &[u8]) -> Result<Bank, BankError> {
    const CUT: BankError = BankError::Wave("the tone file is cut short");
    let mut rest = bytes
        .strip_prefix(MAGIC.as_slice())
        .ok_or(BankError::Wave("not a tone file"))?;
    let mut samples = BTreeMap::new();
    while !rest.is_empty() {
        let (head, tail) = rest.split_at_checked(12).ok_or(CUT)?;
        let word = |i: usize| [head[i], head[i + 1], head[i + 2], head[i + 3]];
        let note = i32::from_le_bytes(word(0));
        let rate_hz = u32::from_le_bytes(word(4));
        let frames = usize::try_from(u32::from_le_bytes(word(8))).map_err(|_| CUT)?;
        if rate_hz == 0 {
            return Err(BankError::Wave("rate"));
        }
        let (body, tail) = tail
            .split_at_checked(frames.checked_mul(2).ok_or(CUT)?)
            .ok_or(CUT)?;
        let pcm = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        samples.insert(note, Sample { rate_hz, pcm });
        rest = tail;
    }
    if samples.is_empty() {
        return Err(BankError::NoResource);
    }
    Ok(Bank::from_samples(samples))
}
