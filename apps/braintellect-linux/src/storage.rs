//! Session storage: every completed session persists to one local SQLite
//! database. Offline only; nothing is read from or migrated out of the
//! original's MySQL store.
//!
//! The schema is normalized and keeps the fields and meaning the original
//! recorded per session (`docs/reverse-engineering/braintellect2/session-storage.md`:
//! the exercise header, the `exercise.duration`, and the per-channel feedback
//! series), not its byte layout or Java-serialized, encrypted blobs. The schema
//! version is `PRAGMA user_version` ([`SCHEMA_VERSION`]); a database with another
//! version is refused, not migrated.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::feedback::circle_balance::{CircleBalanceDesign, Lmh, LmhBand, LmhPoint};

/// Version of the table layout below, stored as `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = "
CREATE TABLE session (
    id INTEGER PRIMARY KEY,
    version TEXT NOT NULL,
    design TEXT NOT NULL,
    montage TEXT NOT NULL,
    device TEXT NOT NULL,
    samples_per_second INTEGER NOT NULL,
    samples_per_packet INTEGER NOT NULL,
    number_channels INTEGER NOT NULL,
    device_gain REAL NOT NULL,
    sensor_gain REAL NOT NULL,
    device_family INTEGER NOT NULL,
    device_model TEXT NOT NULL,
    interference_filter TEXT NOT NULL,
    device_high_pass_cutoff REAL NOT NULL,
    device_low_pass_cutoff REAL NOT NULL,
    data_storage_style TEXT NOT NULL,
    duration_s INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    capture TEXT,
    test INTEGER NOT NULL DEFAULT 0 CHECK (test IN (0, 1))
);
CREATE TABLE channel (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    UNIQUE (session_id, position)
);
CREATE TABLE series (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    UNIQUE (channel_id, position)
);
CREATE TABLE point (
    series_id INTEGER NOT NULL REFERENCES series(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    PRIMARY KEY (series_id, position)
);
";

/// The exercise header the original stored with each session.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub version: String,
    pub design: String,
    pub montage: String,
    pub device: String,
    pub samples_per_second: u32,
    pub samples_per_packet: u32,
    pub number_channels: u32,
    pub device_gain: f64,
    pub sensor_gain: f64,
    pub device_family: u32,
    pub device_model: String,
    pub interference_filter: String,
    pub device_high_pass_cutoff: f64,
    pub device_low_pass_cutoff: f64,
    pub data_storage_style: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A named feedback series (a band, for example) of one channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub name: String,
    pub series: Vec<Series>,
}

/// One completed session.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRecord {
    pub header: Header,
    pub duration_s: u32,
    /// When the session started, in seconds since the Unix epoch.
    pub started_at: i64,
    /// The session's raw capture file, relative to the data directory
    /// (`captures/b2-<secs>.rx`, [`crate::capture`]); `None` when none was written.
    pub capture: Option<String>,
    pub channels: Vec<Channel>,
}

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The database was written with a different [`SCHEMA_VERSION`].
    SchemaVersion {
        found: i64,
        expected: i64,
    },
    NotFound(i64),
}

/// One stored session as the history lists it: when, how long, whether it was
/// marked as a test, the mean of each channel's band series, and its Balance Meter.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub id: i64,
    pub started_at: i64,
    pub duration_s: u32,
    /// Marked as a test run, to keep it out of research data.
    pub test: bool,
    /// Channel and series names (`Band-00` to `Band-VHF`) and the mean of the series' points.
    pub bands: Vec<BandMean>,
    /// The Circle-Balance report over the stored `LMH` series, in
    /// [`CircleBalanceDesign`] input order; `None` where a window keeps no point.
    pub balance: Vec<Option<f32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandMean {
    pub channel: String,
    pub series: String,
    pub mean: f64,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sqlite(e) => write!(f, "session database: {e}"),
            Error::Io(e) => write!(f, "session database location: {e}"),
            Error::SchemaVersion { found, expected } => write!(
                f,
                "session database has schema version {found}, expected {expected}"
            ),
            Error::NotFound(id) => write!(f, "no stored session {id}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// The clone's data directory by XDG convention:
/// `$XDG_DATA_HOME/braintellect-linux`, else
/// `$HOME/.local/share/braintellect-linux`. A relative `XDG_DATA_HOME` is
/// ignored, as the XDG spec requires. `None` when neither is set.
pub fn resolve_data_dir(
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let base = match xdg_data_home
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    {
        Some(dir) => dir,
        None => PathBuf::from(home.filter(|v| !v.is_empty())?).join(".local/share"),
    };
    Some(base.join("braintellect-linux"))
}

/// Why [`default_data_dir`] and the paths under it are `None`.
pub const NO_DATA_DIR: &str = "neither XDG_DATA_HOME nor HOME is set";

/// [`resolve_data_dir`] over the process environment.
pub fn default_data_dir() -> Option<PathBuf> {
    resolve_data_dir(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

/// The session database location: `sessions.db` in the data directory. Pass an
/// explicit path to [`Store::open`] to override it.
pub fn resolve_path(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    Some(resolve_data_dir(xdg_data_home, home)?.join("sessions.db"))
}

/// [`resolve_path`] over the process environment.
pub fn default_path() -> Option<PathBuf> {
    resolve_path(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

/// The local session database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open the database at `path`, creating it (and its directory) when new.
    pub fn open(path: &Path) -> Result<Store, Error> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let found: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match found {
            0 => {
                conn.execute_batch(&format!(
                    "BEGIN; {SCHEMA} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
                ))?;
            }
            SCHEMA_VERSION => {}
            _ => {
                return Err(Error::SchemaVersion {
                    found,
                    expected: SCHEMA_VERSION,
                })
            }
        }
        Ok(Store { conn })
    }

    /// Persist a completed session; returns its id.
    pub fn save(&self, s: &SessionRecord) -> Result<i64, Error> {
        let tx = self.conn.unchecked_transaction()?;
        let h = &s.header;
        tx.execute(
            "INSERT INTO session (version, design, montage, device, samples_per_second,
                samples_per_packet, number_channels, device_gain, sensor_gain, device_family,
                device_model, interference_filter, device_high_pass_cutoff,
                device_low_pass_cutoff, data_storage_style, duration_s, started_at, capture)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                h.version,
                h.design,
                h.montage,
                h.device,
                h.samples_per_second,
                h.samples_per_packet,
                h.number_channels,
                h.device_gain,
                h.sensor_gain,
                h.device_family,
                h.device_model,
                h.interference_filter,
                h.device_high_pass_cutoff,
                h.device_low_pass_cutoff,
                h.data_storage_style,
                s.duration_s,
                s.started_at,
                s.capture,
            ],
        )?;
        let session_id = tx.last_insert_rowid();
        for (ci, channel) in s.channels.iter().enumerate() {
            tx.execute(
                "INSERT INTO channel (session_id, position, name) VALUES (?1,?2,?3)",
                params![session_id, ci as i64, channel.name],
            )?;
            let channel_id = tx.last_insert_rowid();
            for (si, series) in channel.series.iter().enumerate() {
                tx.execute(
                    "INSERT INTO series (channel_id, position, name) VALUES (?1,?2,?3)",
                    params![channel_id, si as i64, series.name],
                )?;
                let series_id = tx.last_insert_rowid();
                for (pi, p) in series.points.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO point (series_id, position, x, y) VALUES (?1,?2,?3,?4)",
                        params![series_id, pi as i64, p.x, p.y],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(session_id)
    }

    /// Ids of every stored session, oldest first.
    pub fn list(&self) -> Result<Vec<i64>, Error> {
        let mut stmt = self.conn.prepare("SELECT id FROM session ORDER BY id")?;
        let ids = stmt.query_map([], |r| r.get(0))?;
        Ok(ids.collect::<Result<_, _>>()?)
    }

    /// Every stored session, newest first, with its band means and Balance Meter.
    pub fn summaries(&self) -> Result<Vec<Summary>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, duration_s, test FROM session ORDER BY started_at DESC, id DESC",
        )?;
        let mut sessions: Vec<Summary> = stmt
            .query_map([], |r| {
                Ok(Summary {
                    id: r.get(0)?,
                    started_at: r.get(1)?,
                    duration_s: r.get(2)?,
                    test: r.get(3)?,
                    bands: Vec::new(),
                    balance: Vec::new(),
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut bands = self.conn.prepare(
            "SELECT c.name, se.name, AVG(p.y) FROM channel c
             JOIN series se ON se.channel_id = c.id
             JOIN point p ON p.series_id = se.id
             WHERE c.session_id = ?1 AND se.name LIKE 'Band-%'
             GROUP BY se.id ORDER BY c.position, se.position",
        )?;
        let mut lmh_points = self.conn.prepare(
            "SELECT c.position, se.name, p.x, p.y FROM channel c
             JOIN series se ON se.channel_id = c.id
             JOIN point p ON p.series_id = se.id
             WHERE c.session_id = ?1 AND c.position < 4
               AND (se.name LIKE 'High %' OR se.name LIKE 'Low %')
             ORDER BY c.position, se.position, p.position",
        )?;
        let design = CircleBalanceDesign::circle_balance();
        for s in &mut sessions {
            s.bands = bands
                .query_map([s.id], |r| {
                    Ok(BandMean {
                        channel: r.get(0)?,
                        series: r.get(1)?,
                        mean: r.get(2)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            let mut lmh = Lmh::default();
            let mut rows = lmh_points.query([s.id])?;
            while let Some(r) = rows.next()? {
                let (position, name, x, y): (i64, String, f64, f64) =
                    (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
                let band = if name.starts_with("High") {
                    LmhBand::High
                } else {
                    LmhBand::Low
                };
                lmh.series_mut(position as usize + 1, band).push(LmhPoint {
                    x_ms: x as i64,
                    y: y as f32,
                });
            }
            s.balance = design.report(&lmh, s.duration_s);
        }
        Ok(sessions)
    }

    /// Mark a stored session as a test run, or clear the mark.
    pub fn set_test(&self, id: i64, test: bool) -> Result<(), Error> {
        match self.conn.execute(
            "UPDATE session SET test = ?1 WHERE id = ?2",
            params![test, id],
        )? {
            0 => Err(Error::NotFound(id)),
            _ => Ok(()),
        }
    }

    /// Read a stored session back.
    pub fn load(&self, id: i64) -> Result<SessionRecord, Error> {
        let (header, duration_s, started_at, capture) = self
            .conn
            .query_row(
                "SELECT version, design, montage, device, samples_per_second,
                    samples_per_packet, number_channels, device_gain, sensor_gain,
                    device_family, device_model, interference_filter,
                    device_high_pass_cutoff, device_low_pass_cutoff, data_storage_style,
                    duration_s, started_at, capture
                 FROM session WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        Header {
                            version: r.get(0)?,
                            design: r.get(1)?,
                            montage: r.get(2)?,
                            device: r.get(3)?,
                            samples_per_second: r.get(4)?,
                            samples_per_packet: r.get(5)?,
                            number_channels: r.get(6)?,
                            device_gain: r.get(7)?,
                            sensor_gain: r.get(8)?,
                            device_family: r.get(9)?,
                            device_model: r.get(10)?,
                            interference_filter: r.get(11)?,
                            device_high_pass_cutoff: r.get(12)?,
                            device_low_pass_cutoff: r.get(13)?,
                            data_storage_style: r.get(14)?,
                        },
                        r.get(15)?,
                        r.get(16)?,
                        r.get(17)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound(id))?;

        let mut channels = Vec::new();
        let mut channel_stmt = self
            .conn
            .prepare("SELECT id, name FROM channel WHERE session_id = ?1 ORDER BY position")?;
        let channel_rows: Vec<(i64, String)> = channel_stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (channel_id, name) in channel_rows {
            let mut series_stmt = self
                .conn
                .prepare("SELECT id, name FROM series WHERE channel_id = ?1 ORDER BY position")?;
            let series_rows: Vec<(i64, String)> = series_stmt
                .query_map([channel_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let mut series = Vec::new();
            for (series_id, name) in series_rows {
                let mut point_stmt = self
                    .conn
                    .prepare("SELECT x, y FROM point WHERE series_id = ?1 ORDER BY position")?;
                let points = point_stmt
                    .query_map([series_id], |r| {
                        Ok(Point {
                            x: r.get(0)?,
                            y: r.get(1)?,
                        })
                    })?
                    .collect::<Result<_, _>>()?;
                series.push(Series { name, points });
            }
            channels.push(Channel { name, series });
        }
        Ok(SessionRecord {
            header,
            duration_s,
            started_at,
            capture,
            channels,
        })
    }
}
