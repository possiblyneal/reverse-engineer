//! A capture read back in order as a transport; whatever the session writes is dropped.

use std::io::{self, Cursor, Read, Write};

pub struct Replayed(pub Cursor<Vec<u8>>);

impl Read for Replayed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Replayed {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
