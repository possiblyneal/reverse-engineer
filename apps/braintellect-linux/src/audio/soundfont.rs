//! A SoundFont 2 reader and note renderer, for the recorded feedback tones
//! (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Recorded tones").
//!
//! Only what a General MIDI melodic preset needs is read: the first preset zone's
//! instrument, its zones' key range, sample, root key, tuning, scale tuning and
//! loop mode, with the instrument's global zone as defaults. Preset-level
//! generators, velocity ranges, envelopes, filters, pan, modulators and the
//! 24-bit sample extension are ignored. Right-channel halves of stereo pairs are
//! skipped, so a tone is the left (or mono) recording.

use std::fmt;

const GEN_KEY_RANGE: u16 = 43;
const GEN_INSTRUMENT: u16 = 41;
const GEN_COARSE_TUNE: u16 = 51;
const GEN_FINE_TUNE: u16 = 52;
const GEN_SAMPLE_ID: u16 = 53;
const GEN_SAMPLE_MODES: u16 = 54;
const GEN_SCALE_TUNING: u16 = 56;
const GEN_ROOT_KEY: u16 = 58;

const PHDR_LEN: usize = 38;
const BAG_LEN: usize = 4;
const GEN_LEN: usize = 4;
const INST_LEN: usize = 22;
const SHDR_LEN: usize = 46;
/// Sample type bits: the right half of a stereo pair, and a ROM sample.
const TYPE_RIGHT: u16 = 2;
const TYPE_ROM: u16 = 0x8000;
/// `shdr` original pitch meaning "unpitched"; the root key falls back to middle C.
const UNPITCHED: u8 = 255;
const MIDDLE_C: i32 = 60;
/// Cents per key when `scaleTuning` is not set.
const DEFAULT_SCALE_CENTS: i32 = 100;
/// Output frames faded to silence at the end of a rendered note, so a note cut
/// at its length does not click (5 ms at 96 kHz).
const FADE_FRAMES: usize = 480;

/// Why a SoundFont could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum SoundFontError {
    /// Not a SoundFont 2 file, or a table is cut short or points outside the file.
    Malformed(&'static str),
    /// The font has no bank 0 preset with this General MIDI program number.
    MissingPreset(u16),
    /// The preset has no instrument with a usable sample.
    NoSample(u16),
}

impl fmt::Display for SoundFontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoundFontError::Malformed(why) => write!(f, "not a usable SoundFont 2 file: {why}"),
            SoundFontError::MissingPreset(program) => {
                write!(f, "the SoundFont has no General MIDI program {program}")
            }
            SoundFontError::NoSample(program) => {
                write!(f, "General MIDI program {program} has no usable sample")
            }
        }
    }
}

impl std::error::Error for SoundFontError {}

/// One sample of a preset, with where it plays and how it is tuned.
#[derive(Debug, Clone)]
pub struct Zone {
    /// Lowest and highest MIDI key the zone plays.
    pub keys: (u8, u8),
    pub rate_hz: u32,
    /// The key the sample sounds at, before `cents`.
    pub root_key: i32,
    /// Coarse, fine and sample-header tuning, added to the pitch.
    pub cents: i32,
    /// Cents per key step.
    pub scale_cents: i32,
    /// Loop points as indexes into `pcm`, when the sample sustains by looping.
    pub looped: Option<(usize, usize)>,
    pub pcm: Vec<i16>,
}

/// The tables of a SoundFont, read from its bytes.
pub struct SoundFont {
    smpl: Vec<i16>,
    phdr: Vec<u8>,
    pbag: Vec<u8>,
    pgen: Vec<u8>,
    inst: Vec<u8>,
    ibag: Vec<u8>,
    igen: Vec<u8>,
    shdr: Vec<u8>,
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

/// A chunk's four-byte id and its body.
type Chunk<'a> = (&'a [u8], &'a [u8]);

/// The chunks laid end to end in `bytes`.
fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, SoundFontError> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let len = le32(bytes, at + 4);
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or(SoundFontError::Malformed("chunk past the end"))?;
        out.push((&bytes[at..at + 4], body));
        at += 8 + len + (len & 1);
    }
    Ok(out)
}

impl SoundFont {
    /// Read the sample data and the preset tables of a SoundFont 2 file.
    ///
    /// # Errors
    /// [`SoundFontError::Malformed`] when the bytes are not a SoundFont 2 file or a
    /// table is missing or cut short.
    pub fn parse(bytes: &[u8]) -> Result<SoundFont, SoundFontError> {
        let body = match chunks(bytes)?.as_slice() {
            [(b"RIFF", body), ..] if body.starts_with(b"sfbk") => &body[4..],
            _ => return Err(SoundFontError::Malformed("no RIFF sfbk header")),
        };
        let (mut smpl, mut pdta) = (None, None);
        for (id, list) in chunks(body)? {
            if id != b"LIST" || list.len() < 4 {
                continue;
            }
            match &list[..4] {
                b"sdta" => {
                    smpl = chunks(&list[4..])?
                        .into_iter()
                        .find(|(id, _)| *id == b"smpl")
                        .map(|(_, data)| data);
                }
                b"pdta" => pdta = Some(chunks(&list[4..])?),
                _ => {}
            }
        }
        let smpl = smpl.ok_or(SoundFontError::Malformed("no sample data"))?;
        let pdta = pdta.ok_or(SoundFontError::Malformed("no preset data"))?;
        let table = |name: &[u8; 4], record: usize| -> Result<Vec<u8>, SoundFontError> {
            let body = pdta
                .iter()
                .find(|(id, _)| *id == name)
                .map(|(_, body)| *body)
                .ok_or(SoundFontError::Malformed("a preset table is missing"))?;
            if body.len() % record != 0 || body.len() < record {
                return Err(SoundFontError::Malformed("a preset table is cut short"));
            }
            Ok(body.to_vec())
        };
        Ok(SoundFont {
            smpl: smpl
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect(),
            phdr: table(b"phdr", PHDR_LEN)?,
            pbag: table(b"pbag", BAG_LEN)?,
            pgen: table(b"pgen", GEN_LEN)?,
            inst: table(b"inst", INST_LEN)?,
            ibag: table(b"ibag", BAG_LEN)?,
            igen: table(b"igen", GEN_LEN)?,
            shdr: table(b"shdr", SHDR_LEN)?,
        })
    }

    /// The zones of the bank 0 preset with General MIDI `program`, in file order.
    ///
    /// # Errors
    /// [`SoundFontError::MissingPreset`] when the font has no such preset,
    /// [`SoundFontError::NoSample`] when it plays no sample, and
    /// [`SoundFontError::Malformed`] when an index points outside its table.
    pub fn zones(&self, program: u16) -> Result<Vec<Zone>, SoundFontError> {
        // The last record of `phdr` is the terminator.
        let preset = (0..self.phdr.len() / PHDR_LEN - 1)
            .find(|&i| {
                le16(&self.phdr, i * PHDR_LEN + 20) == program
                    && le16(&self.phdr, i * PHDR_LEN + 22) == 0
            })
            .ok_or(SoundFontError::MissingPreset(program))?;
        let (first_bag, end_bag) = index_range(&self.phdr, PHDR_LEN, 24, preset)?;
        let mut instrument = None;
        for bag in first_bag..end_bag {
            let range = index_range(&self.pbag, BAG_LEN, 0, bag)?;
            if let Some((_, id)) = generators(&self.pgen, range)?
                .into_iter()
                .find(|(op, _)| *op == GEN_INSTRUMENT)
            {
                instrument = Some(usize::from(id as u16));
                break;
            }
        }
        let instrument = instrument.ok_or(SoundFontError::NoSample(program))?;

        let (first_zone, end_zone) = index_range(&self.inst, INST_LEN, 20, instrument)?;
        let mut global: Vec<(u16, i16)> = Vec::new();
        let mut zones = Vec::new();
        for (n, bag) in (first_zone..end_zone).enumerate() {
            let gens = generators(&self.igen, index_range(&self.ibag, BAG_LEN, 0, bag)?)?;
            if !gens.iter().any(|(op, _)| *op == GEN_SAMPLE_ID) {
                if n == 0 {
                    global = gens;
                }
                continue;
            }
            if let Some(zone) = self.zone(&global, &gens)? {
                zones.push(zone);
            }
        }
        if zones.is_empty() {
            return Err(SoundFontError::NoSample(program));
        }
        Ok(zones)
    }

    /// One instrument zone: `gens` over the instrument's `global` defaults.
    fn zone(
        &self,
        global: &[(u16, i16)],
        gens: &[(u16, i16)],
    ) -> Result<Option<Zone>, SoundFontError> {
        let get = |op: u16| {
            gens.iter()
                .chain(global)
                .find(|(o, _)| *o == op)
                .map(|(_, v)| *v)
        };
        // `sampleID` is unsigned, as `instrument` is.
        let id = usize::from(get(GEN_SAMPLE_ID).unwrap_or(0) as u16);
        let record =
            self.shdr
                .get(id * SHDR_LEN..(id + 1) * SHDR_LEN)
                .ok_or(SoundFontError::Malformed(
                    "a sample index is outside the table",
                ))?;
        let kind = le16(record, 44);
        if kind & (TYPE_RIGHT | TYPE_ROM) != 0 {
            return Ok(None);
        }
        let (start, end) = (le32(record, 20), le32(record, 24));
        let (loop_start, loop_end) = (le32(record, 28), le32(record, 32));
        let pcm = self
            .smpl
            .get(start..end)
            .filter(|p| !p.is_empty())
            .ok_or(SoundFontError::Malformed(
                "a sample lies outside the sample data",
            ))?
            .to_vec();
        let loops = matches!(get(GEN_SAMPLE_MODES).unwrap_or(0) & 3, 1 | 3);
        let looped = (loops && start <= loop_start && loop_start < loop_end && loop_end <= end)
            .then(|| (loop_start - start, loop_end - start));
        let keys = get(GEN_KEY_RANGE).map_or((0, 127), |v| {
            let [lo, hi] = v.to_le_bytes();
            (lo, hi)
        });
        let rate_hz = u32::try_from(le32(record, 36)).unwrap_or(0);
        if rate_hz == 0 {
            return Err(SoundFontError::Malformed("a sample has no rate"));
        }
        let header_pitch = record[40];
        let root_key = get(GEN_ROOT_KEY)
            .filter(|k| (0..=127).contains(k))
            .map_or_else(
                || {
                    if header_pitch == UNPITCHED {
                        MIDDLE_C
                    } else {
                        i32::from(header_pitch)
                    }
                },
                i32::from,
            );
        Ok(Some(Zone {
            keys,
            rate_hz,
            root_key,
            cents: i32::from(get(GEN_COARSE_TUNE).unwrap_or(0)) * 100
                + i32::from(get(GEN_FINE_TUNE).unwrap_or(0))
                + i32::from(record[41] as i8),
            scale_cents: get(GEN_SCALE_TUNING).map_or(DEFAULT_SCALE_CENTS, i32::from),
            looped,
            pcm,
        }))
    }
}

const BAD_INDEX: SoundFontError = SoundFontError::Malformed("an index points outside its table");

/// The `[from, to)` span a table record's u16 `field` and the next record's give,
/// as `table` holds `record`-byte records.
fn index_range(
    table: &[u8],
    record: usize,
    field: usize,
    index: usize,
) -> Result<(usize, usize), SoundFontError> {
    let at = |i: usize| {
        table
            .get(i * record + field..i * record + field + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    match (at(index), at(index + 1)) {
        (Some(from), Some(to)) if from <= to => Ok((from, to)),
        _ => Err(BAD_INDEX),
    }
}

/// The `(operator, amount)` generators `[from, to)` of a generator table.
fn generators(table: &[u8], (from, to): (usize, usize)) -> Result<Vec<(u16, i16)>, SoundFontError> {
    let bytes = table.get(from * GEN_LEN..to * GEN_LEN).ok_or(BAD_INDEX)?;
    Ok(bytes
        .chunks_exact(GEN_LEN)
        .map(|g| (le16(g, 0), le16(g, 2) as i16))
        .collect())
}

/// Render `note` from `zones` as `frames` mono samples at `out_rate_hz`: the
/// first zone whose key range holds the note, resampled by linear interpolation
/// to the note's pitch, looped when the zone loops, and silent after an unlooped
/// sample ends. `None` when no zone plays the note.
#[must_use]
pub fn render_note(zones: &[Zone], note: i32, out_rate_hz: u32, frames: usize) -> Option<Vec<i16>> {
    let zone = zones
        .iter()
        .find(|z| (i32::from(z.keys.0)..=i32::from(z.keys.1)).contains(&note))?;
    let cents = f64::from((note - zone.root_key) * zone.scale_cents + zone.cents);
    let step = f64::from(zone.rate_hz) / f64::from(out_rate_hz) * 2f64.powf(cents / 1200.0);
    let mut out = Vec::with_capacity(frames);
    let mut pos = 0.0f64;
    for _ in 0..frames {
        if let Some((start, end)) = zone.looped {
            let len = (end - start) as f64;
            while pos >= end as f64 {
                pos -= len;
            }
        }
        let i = pos as usize;
        let value = zone.pcm.get(i).map_or(0.0, |&a| {
            let b = zone.pcm.get(i + 1).map_or(0.0, |&b| f64::from(b));
            f64::from(a) + (b - f64::from(a)) * pos.fract()
        });
        out.push(value);
        pos += step;
    }
    let tail = frames.min(FADE_FRAMES);
    for (k, v) in out[frames - tail..].iter_mut().enumerate() {
        *v *= 1.0 - (k + 1) as f64 / tail as f64;
    }
    Some(
        out.into_iter()
            .map(|v| v.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16)
            .collect(),
    )
}
