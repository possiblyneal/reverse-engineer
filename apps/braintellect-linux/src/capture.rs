//! Raw RX capture: every byte the B-2 sends, handshake replies included, is
//! tee'd to a file as it is read. The file is the input of the B-2 emulator
//! ([`crate::emulate`]), which replays the same signal into the original app.
//! Captures hold a session's raw signal, so they live in the clone's data
//! directory, never in Git.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A transport that copies everything it reads to `sink`. Writes pass through.
pub struct Recording<T, W> {
    inner: T,
    sink: W,
}

impl<T, W> Recording<T, W> {
    pub fn new(inner: T, sink: W) -> Self {
        Self { inner, sink }
    }
}

impl<T: Read, W: Write> Read for Recording<T, W> {
    /// A failed capture write fails the read: a silently short capture would
    /// replay as a different signal.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.sink.write_all(&buf[..n])?;
        Ok(n)
    }
}

impl<T: Write, W> Write for Recording<T, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Create a new capture file `captures/b2-<unix seconds>.rx` under the data
/// directory `data_dir` (made if missing) and return it with its path.
///
/// # Errors
/// Fails if the directory or file cannot be created, or the file exists.
pub fn create(data_dir: &Path) -> io::Result<(File, PathBuf)> {
    let dir = data_dir.join("captures");
    fs::create_dir_all(&dir)?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = dir.join(format!("b2-{secs}.rx"));
    let file = File::options().write(true).create_new(true).open(&path)?;
    Ok((file, path))
}
