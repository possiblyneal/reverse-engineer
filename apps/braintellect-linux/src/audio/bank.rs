//! The instrument bank: the note samples embedded in the original's
//! `dll<id>.BT.dll` (`docs/reverse-engineering/braintellect2/audio-feedback.md`,
//! "Wave device and instrument 1002").
//!
//! The DLL carries a Qt resource (rcc): data blobs (`[BE u32 length][RIFF/WAVE]`),
//! a name table (`[BE u16 length][BE u32 hash][UTF-16BE name]`) and a tree of
//! 14-byte nodes (`name_off u32, flags u16, country u16, language u16,
//! data_off u32`, big-endian). Nodes are in name-hash order, so blob order is not
//! note order: a sample's note is the trailing decimal of its resource name
//! (`AcousticStrings40` is MIDI note 40). The parser finds the three tables by
//! their shape, so it needs no per-bank offsets.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

/// Bytes of one tree node.
const NODE_LEN: usize = 14;
/// Bytes before a name entry's characters: length (u16) and hash (u32).
const NAME_HEADER_LEN: usize = 6;
/// Name tables tried, longest first; the real one is the longest by far.
const MAX_NAME_TABLES: usize = 8;
/// A file node (not a directory, not compressed).
const FLAGS_FILE: usize = 0;
/// Shortest `RIFF` + `WAVE` header the parser reads.
const WAV_HEADER_LEN: usize = 12;

/// One note's audio: mono signed 16-bit PCM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub rate_hz: u32,
    pub pcm: Vec<i16>,
}

/// Why a bank could not be loaded.
#[derive(Debug)]
pub enum BankError {
    Io(std::io::Error),
    /// No sample resource (blobs, names and tree that agree) in the bytes.
    NoResource,
    /// A sample is not a plain 16-bit mono PCM WAV file.
    Wave(&'static str),
}

impl fmt::Display for BankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankError::Io(e) => write!(f, "cannot read the instrument bank: {e}"),
            BankError::NoResource => write!(f, "no note samples found in the instrument bank"),
            BankError::Wave(why) => write!(f, "unsupported sample in the instrument bank: {why}"),
        }
    }
}

impl std::error::Error for BankError {}

impl From<std::io::Error> for BankError {
    fn from(e: std::io::Error) -> Self {
        BankError::Io(e)
    }
}

/// The samples of one instrument, by MIDI note.
#[derive(Debug)]
pub struct Bank {
    samples: BTreeMap<i32, Sample>,
}

impl Bank {
    /// Read and parse the bank file at `path`.
    pub fn load(path: &Path) -> Result<Bank, BankError> {
        Bank::parse(&fs::read(path)?)
    }

    /// Parse the bytes of an instrument DLL.
    pub fn parse(bytes: &[u8]) -> Result<Bank, BankError> {
        let data_base = find_data_base(bytes).ok_or(BankError::NoResource)?;
        let files = name_tables(bytes)
            .iter()
            .map(|names| find_tree(bytes, data_base, names))
            .find(|files| !files.is_empty())
            .ok_or(BankError::NoResource)?;
        let mut samples = BTreeMap::new();
        for (name, data_off) in files {
            let note = trailing_number(&name).ok_or(BankError::NoResource)?;
            let blob = blob_at(bytes, data_base, data_off).ok_or(BankError::NoResource)?;
            samples.insert(note, parse_wav(blob)?);
        }
        Ok(Bank { samples })
    }

    /// The sample of MIDI `note`, if the bank has one.
    pub fn sample(&self, note: i32) -> Option<&Sample> {
        self.samples.get(&note)
    }

    /// The notes the bank holds, ascending.
    pub fn notes(&self) -> impl Iterator<Item = i32> + '_ {
        self.samples.keys().copied()
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(usize::from(u16::from_be_bytes([b[0], b[1]])))
}

fn be32(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    usize::try_from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok()
}

fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok()
}

/// The WAV file stored behind a big-endian length at `at`, when the length is
/// the RIFF size plus its 8-byte header.
fn blob_at(bytes: &[u8], base: usize, data_off: usize) -> Option<&[u8]> {
    let at = base.checked_add(data_off)?;
    let len = be32(bytes, at)?;
    let blob = bytes.get(at + 4..(at + 4).checked_add(len)?)?;
    let riff = blob.get(..WAV_HEADER_LEN)?;
    let valid = &riff[..4] == b"RIFF" && &riff[8..] == b"WAVE" && le32(blob, 4)? + 8 == len;
    valid.then_some(blob)
}

/// Where the data section starts: the first valid blob's length prefix.
fn find_data_base(bytes: &[u8]) -> Option<usize> {
    (0..bytes.len().saturating_sub(WAV_HEADER_LEN)).find(|&at| blob_at(bytes, at, 0).is_some())
}

/// The runs of name entries in the file, longest first: a run is entries laid
/// end to end, each a [`name_at`] entry. Returns each run's entries by their
/// offset from the run's start.
fn name_tables(bytes: &[u8]) -> Vec<BTreeMap<usize, String>> {
    let entry_end = |at: usize| -> Option<(usize, String)> {
        let name = name_at(bytes, at)?;
        Some((at + NAME_HEADER_LEN + name.len() * 2, name))
    };
    let mut continuation = std::collections::HashSet::new();
    let mut starts = Vec::new();
    for at in 0..bytes.len() {
        if let Some((end, _)) = entry_end(at) {
            if !continuation.contains(&at) {
                starts.push(at);
            }
            continuation.insert(end);
        }
    }
    let mut tables: Vec<BTreeMap<usize, String>> = starts
        .into_iter()
        .map(|start| {
            let mut table = BTreeMap::new();
            let mut at = start;
            while let Some((end, name)) = entry_end(at) {
                table.insert(at - start, name);
                at = end;
            }
            table
        })
        .collect();
    tables.sort_by_key(|t| std::cmp::Reverse(t.len()));
    tables.truncate(MAX_NAME_TABLES);
    tables
}

/// The longest run of consecutive file nodes whose name offset lands on an
/// entry of `names` and whose data offset points at a blob, as (name, data
/// offset).
fn find_tree(
    bytes: &[u8],
    data_base: usize,
    names: &BTreeMap<usize, String>,
) -> Vec<(String, usize)> {
    let node_at = |at: usize| -> Option<(String, usize)> {
        let name = names.get(&be32(bytes, at)?)?;
        let data_off = be32(bytes, at + 10)?;
        (be16(bytes, at + 4)? == FLAGS_FILE && blob_at(bytes, data_base, data_off).is_some())
            .then(|| (name.clone(), data_off))
    };
    let mut best = Vec::new();
    for at in 0..bytes.len().saturating_sub(NODE_LEN) {
        let mut run = Vec::new();
        while let Some(node) = node_at(at + run.len() * NODE_LEN) {
            // Zero padding reads as the same node over and over; a tree names
            // each file once.
            if run.iter().any(|(name, _)| *name == node.0) {
                break;
            }
            run.push(node);
        }
        if run.len() > best.len() {
            best = run;
        }
    }
    best
}

/// A name entry: ASCII letters and digits ending in a digit.
fn name_at(bytes: &[u8], at: usize) -> Option<String> {
    let chars = be16(bytes, at)?;
    if !(2..=64).contains(&chars) {
        return None;
    }
    let raw = bytes.get(at + 6..at + 6 + chars * 2)?;
    let name: String = raw
        .chunks(2)
        .map(|c| (c[0] == 0 && c[1].is_ascii_alphanumeric()).then_some(char::from(c[1])))
        .collect::<Option<_>>()?;
    name.ends_with(|c: char| c.is_ascii_digit()).then_some(name)
}

fn trailing_number(name: &str) -> Option<i32> {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    name[name.len() - digits..].parse().ok()
}

/// Parse a plain PCM WAV file: mono, 16 bits.
fn parse_wav(file: &[u8]) -> Result<Sample, BankError> {
    let mut rate_hz = None;
    let mut at = WAV_HEADER_LEN;
    while let Some(header) = file.get(at..at + 8) {
        let len = le32(header, 4).ok_or(BankError::Wave("chunk size"))?;
        let body = file
            .get(
                at + 8
                    ..(at + 8)
                        .checked_add(len)
                        .ok_or(BankError::Wave("chunk size"))?,
            )
            .ok_or(BankError::Wave("truncated chunk"))?;
        match &header[..4] {
            b"fmt " => {
                let word = |i: usize| body.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
                if body.len() < 16 || word(0) != Some(1) {
                    return Err(BankError::Wave("not PCM"));
                }
                if word(2) != Some(1) {
                    return Err(BankError::Wave("not mono"));
                }
                if word(14) != Some(16) {
                    return Err(BankError::Wave("not 16-bit"));
                }
                rate_hz = Some(
                    u32::try_from(le32(body, 4).ok_or(BankError::Wave("rate"))?)
                        .map_err(|_| BankError::Wave("rate"))?,
                );
            }
            b"data" => {
                let rate_hz = rate_hz.ok_or(BankError::Wave("data before fmt"))?;
                let pcm = body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b))
                    .collect();
                return Ok(Sample { rate_hz, pcm });
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    Err(BankError::Wave("no data chunk"))
}
