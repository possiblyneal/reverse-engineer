//! A scripted stand-in for the original app's end of a serial link, for driving
//! the B-2 emulator over `Read + Write`. It judges nothing: the emulator's
//! output is recorded and the test asserts on it.

use std::collections::VecDeque;
use std::io::{self, Read, Write};

pub enum Step {
    /// Bytes the app writes to the controller.
    Input(Vec<u8>),
    /// A read that times out: the link is quiet for one loop turn.
    Quiet,
}

pub struct ScriptedPeer {
    script: VecDeque<Step>,
    /// Everything the emulator wrote back, in order.
    pub received: Vec<u8>,
}

impl ScriptedPeer {
    /// After the last step the peer closes the link (`Ok(0)`).
    pub fn new(script: Vec<Step>) -> Self {
        Self {
            script: script.into(),
            received: Vec::new(),
        }
    }
}

impl Read for ScriptedPeer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.script.pop_front() {
            None => Ok(0),
            Some(Step::Quiet) => Err(io::ErrorKind::TimedOut.into()),
            Some(Step::Input(mut bytes)) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                if n < bytes.len() {
                    bytes.drain(..n);
                    self.script.push_front(Step::Input(bytes));
                }
                Ok(n)
            }
        }
    }
}

impl Write for ScriptedPeer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.received.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
