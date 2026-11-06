//! Importing the original's instrument banks.
//!
//! The banks are the original's own `dll<id>.BT.dll` files, vendor content that
//! never goes into Git. The import command copies them from the original's
//! `lib` directory into the clone's data directory, where the player loads them.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::storage;

/// Why an import failed.
#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    /// The source directory holds no `dll<id>.BT.dll`.
    NoBanks,
    /// The source and destination are the same directory; copying would
    /// truncate each bank onto itself.
    SameDirectory,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "{e}"),
            ImportError::NoBanks => write!(f, "no dll<id>.BT.dll instrument files found"),
            ImportError::SameDirectory => {
                write!(f, "the source is the instruments directory itself; give the original install's lib directory")
            }
        }
    }
}

impl std::error::Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        ImportError::Io(e)
    }
}

/// The instruments directory by the storage layer's XDG rule:
/// `<data dir>/instruments`.
pub fn default_dir() -> Option<PathBuf> {
    Some(storage::default_data_dir()?.join("instruments"))
}

/// Where instrument `id`'s bank lives in `dir`.
pub fn bank_path(dir: &Path, id: u32) -> PathBuf {
    dir.join(format!("dll{id}.BT.dll"))
}

/// Whether `name` is `dll<digits>.BT.dll`.
fn is_bank_file(name: &str) -> bool {
    name.strip_prefix("dll")
        .and_then(|rest| rest.strip_suffix(".BT.dll"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Copy every `dll<id>.BT.dll` of `lib_dir` into `dest` (created when new),
/// overwriting older copies. Returns the destination paths, sorted.
pub fn import_banks(lib_dir: &Path, dest: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut names: Vec<OsString> = fs::read_dir(lib_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name())
        .filter(|name| name.to_str().is_some_and(is_bank_file))
        .collect();
    if names.is_empty() {
        return Err(ImportError::NoBanks);
    }
    names.sort();
    fs::create_dir_all(dest)?;
    if fs::canonicalize(lib_dir)? == fs::canonicalize(dest)? {
        return Err(ImportError::SameDirectory);
    }
    names
        .into_iter()
        .map(|name| {
            let target = dest.join(&name);
            fs::copy(lib_dir.join(&name), &target)?;
            Ok(target)
        })
        .collect()
}
