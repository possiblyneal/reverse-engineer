//! Audio output: the instrument bank, the deterministic player and the ALSA
//! sink (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Wave
//! device and instrument 1002").
//!
//! Only [`sink`] touches an audio device. The bank is the original's own
//! `dll<id>.BT.dll`, imported once by [`import`] into the user's data
//! directory; it never lives in Git.

pub mod bank;
pub mod import;
pub mod player;
pub mod sink;
pub mod soundfont;
pub mod tone;

use std::fmt;
use std::path::{Path, PathBuf};

use bank::{Bank, BankError};

/// Why an instrument could not be loaded for a session.
#[derive(Debug)]
pub enum InstrumentError {
    /// The instrument's bank is not in the instruments directory: it was never
    /// imported.
    Missing(PathBuf),
    /// The bank is there but unusable.
    Bank(PathBuf, BankError),
}

impl fmt::Display for InstrumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentError::Missing(path) => write!(
                f,
                "the instrument bank {} is missing; run `braintellect-linux import <original lib dir>` \
                 (original banks) or `braintellect-linux import-soundfont <file.sf2>` (recorded tones)",
                path.display()
            ),
            InstrumentError::Bank(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

impl std::error::Error for InstrumentError {}

/// Load instrument `id` from the instruments directory `dir`. A bank that was
/// never imported is [`InstrumentError::Missing`], never a silent fallback.
pub fn load_instrument(dir: &Path, id: u32) -> Result<Bank, InstrumentError> {
    let path = import::bank_path(dir, id);
    Bank::load(&path).map_err(|e| match e {
        BankError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            InstrumentError::Missing(path)
        }
        e => InstrumentError::Bank(path, e),
    })
}
