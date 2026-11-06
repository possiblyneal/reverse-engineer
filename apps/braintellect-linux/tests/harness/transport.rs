//! In-memory stand-in for the RFCOMM socket, driven by the baseline fixtures.
//!
//! Scripted from the wire fixture: after the clone's k-th write, the replies
//! the real app received after its k-th write become readable. After the last
//! scripted write (the start command) the stream bytes become readable, in the
//! chunks the capture delivered them in. It judges nothing: the clone's writes
//! are recorded and the harness assertions compare them to the baseline.

use std::collections::VecDeque;
use std::io::{self, Read, Write};

use super::fixtures::{Dir, WireEvent};

pub struct ReplayTransport {
    /// Replies to deliver after each scripted write, in write order.
    replies: VecDeque<Vec<Vec<u8>>>,
    /// Stream chunks delivered once the last scripted write has happened.
    stream: Vec<Vec<u8>>,
    readable: VecDeque<Vec<u8>>,
    /// Report a read timeout instead of end of stream once nothing is left.
    stall: bool,
    pub written: Vec<Vec<u8>>,
}

impl ReplayTransport {
    pub fn new(wire: Vec<WireEvent>, stream: Vec<Vec<u8>>) -> Self {
        let mut replies: VecDeque<Vec<Vec<u8>>> = VecDeque::new();
        for event in wire {
            match event.dir {
                Dir::Tx => replies.push_back(Vec::new()),
                Dir::Rx => replies.back_mut().expect("RX after TX").push(event.bytes),
            }
        }
        Self {
            replies,
            stream,
            readable: VecDeque::new(),
            stall: false,
            written: Vec::new(),
        }
    }
}

impl ReplayTransport {
    /// Once the stream is used up, reads time out (a quiet link) instead of
    /// reporting end of stream.
    pub fn stalling(mut self) -> Self {
        self.stall = true;
        self
    }
}

/// Wire frames (`stream.txt` layout: marker, counter, `03 03`, eight 24-bit
/// samples) carrying raw ADC counts, one frame per row, counters from 0.
pub fn frames_from_counts(rows: &[[i32; 8]]) -> Vec<Vec<u8>> {
    rows.iter()
        .enumerate()
        .map(|(n, row)| {
            let mut frame = vec![0xAC, 0xBC, 0xCC, 0x03];
            frame.extend(u16::try_from(n % 65536).expect("counter").to_be_bytes());
            frame.extend([3, 3]);
            frame.extend(row.iter().flat_map(|v| v.to_be_bytes()[1..].to_vec()));
            frame
        })
        .collect()
}

impl Write for ReplayTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.push(buf.to_vec());
        if let Some(replies) = self.replies.pop_front() {
            self.readable.extend(replies);
            if self.replies.is_empty() {
                self.readable.extend(std::mem::take(&mut self.stream));
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for ReplayTransport {
    /// One scripted chunk per call (or part of it); `Ok(0)` once nothing is left.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(mut chunk) = self.readable.pop_front() else {
            return if self.stall {
                Err(io::ErrorKind::TimedOut.into())
            } else {
                Ok(0)
            };
        };
        let n = chunk.len().min(buf.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        if n < chunk.len() {
            chunk.drain(..n);
            self.readable.push_front(chunk);
        }
        Ok(n)
    }
}
