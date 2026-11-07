//! A connected B-2 session over any byte stream (`Read + Write`): handshake,
//! start/stop, and the decoded sample stream.
//!
//! The command sequence is the one the real app wrote in the 2026-11-04 trace
//! (`wire.txt` fixture): stop, a query pass, mode and signal-injection setup,
//! a second query pass; then, on start, three stops and the start command.

use std::fmt;
use std::io::{self, Read, Write};

use super::command::{
    Command, REQUEST_CHANNEL_INFO, REQUEST_SERIAL, REQUEST_SYSTEM_INFO_1, REQUEST_SYSTEM_INFO_2,
};
use super::decode::{ChannelCalibration, DeviceFamily};
use super::info::DeviceInfo;
use super::stream::{
    decode_payload, FrameParser, SampleFrame, StreamConfig, StreamStats, FRAME_LEN,
};

/// Handshake replies are exactly 56 bytes (`b2-protocol.md`, reply layout).
pub const REPLY_LEN: usize = 56;
const REPLY_MARKER: [u8; 4] = [0xAC, 0xBC, 0xCC, 0x03];
/// Serial reply: `[8]` echoes the code, then six 2-byte pairs each followed by a
/// `03` separator from `[10]` (12-byte id).
const SERIAL_START: usize = 10;
const SERIAL_PAIRS: usize = 6;
/// The app wrote `setStop` three times before `setStart` (observed).
const STOPS_BEFORE_START: usize = 3;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// A reply lacked the marker or did not echo the request code.
    BadReply {
        code: u8,
    },
    /// The B-2 reported a device family the app does not accept (it takes 11, 12
    /// and 99), and the caller supplied no override.
    UnknownFamily(u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "transport error: {e}"),
            Self::BadReply { code } => write!(f, "malformed reply to request {code}"),
            Self::UnknownFamily(code) => write!(f, "the B-2 reports unknown device family {code}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A session whose handshake has completed: holding one is the ready state.
pub struct Session<T> {
    transport: T,
    calibration: ChannelCalibration,
    family: DeviceFamily,
    info: DeviceInfo,
    serial: [u8; SERIAL_PAIRS * 2],
    parser: FrameParser,
}

impl<T: Read + Write> Session<T> {
    /// Run the handshake over `transport`. The device family is the one the B-2
    /// reports in its 110 reply unless `config.family` overrides it.
    ///
    /// # Errors
    /// Fails on a transport error, a reply that is not the expected 56-byte frame
    /// echoing its request, or a reported family the app does not accept with no
    /// override.
    pub fn open(mut transport: T, config: StreamConfig) -> Result<Self, Error> {
        send(&mut transport, Command::Stop)?;
        query_pass(&mut transport)?;
        send(&mut transport, Command::SetModeNormalEeg)?;
        send(&mut transport, Command::SetSignalInjectionOff)?;
        let (serial, info) = query_pass(&mut transport)?;
        let family = match config.family {
            Some(family) => family,
            None => info.family().map_err(Error::UnknownFamily)?,
        };
        Ok(Self {
            transport,
            calibration: config.calibration,
            family,
            info,
            serial,
            parser: FrameParser::default(),
        })
    }

    /// The device serial number from the 101 reply.
    #[must_use]
    pub fn serial(&self) -> &[u8] {
        &self.serial
    }

    /// What the 110, 112 and 131-138 replies of the last query pass said.
    #[must_use]
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// The device family samples are converted with: the B-2's, or the caller's
    /// override.
    #[must_use]
    pub fn family(&self) -> DeviceFamily {
        self.family
    }

    /// Start streaming with `channels` active channels.
    ///
    /// # Errors
    /// Fails on a transport error.
    pub fn start(&mut self, channels: u8) -> Result<(), Error> {
        for _ in 0..STOPS_BEFORE_START {
            send(&mut self.transport, Command::Stop)?;
        }
        send(&mut self.transport, Command::Start { channels })
    }

    /// The next valid decoded frame, or `None` when the stream has ended.
    /// Counter and validation errors accumulate in [`Self::stats`].
    ///
    /// # Errors
    /// Fails on a transport error.
    pub fn next_frame(&mut self) -> Result<Option<SampleFrame>, Error> {
        let mut chunk = [0u8; 256];
        loop {
            if let Some((counter, payload)) = self.parser.next_frame() {
                return Ok(Some(SampleFrame {
                    counter,
                    channels: decode_payload(&payload, self.calibration, self.family),
                }));
            }
            match self.transport.read(&mut chunk) {
                Ok(0) => return Ok(None),
                Ok(n) => self.parser.push(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    #[must_use]
    pub fn stats(&self) -> StreamStats {
        self.parser.stats()
    }

    /// Stop streaming and hand the transport back to the caller to close.
    ///
    /// # Errors
    /// Fails if the stop command cannot be written.
    pub fn disconnect(mut self) -> Result<T, Error> {
        send(&mut self.transport, Command::Stop)?;
        Ok(self.transport)
    }
}

fn send<T: Write>(transport: &mut T, command: Command) -> Result<(), Error> {
    transport.write_all(&command.encode())?;
    transport.flush()?;
    Ok(())
}

/// Send request `code` and read its 56-byte reply. Stream frames still
/// arriving from a B-2 left streaming (a 32-byte frame with `03 03` at `[6]`
/// and the next marker right after it) are skipped first.
fn request<T: Read + Write>(transport: &mut T, code: u8) -> Result<[u8; REPLY_LEN], Error> {
    send(transport, Command::Request(code))?;
    let mut reply = [0u8; REPLY_LEN];
    transport.read_exact(&mut reply)?;
    while reply[..4] == REPLY_MARKER
        && reply[6..8] == [3, 3]
        && reply[FRAME_LEN..FRAME_LEN + 4] == REPLY_MARKER
    {
        reply.copy_within(FRAME_LEN.., 0);
        transport.read_exact(&mut reply[REPLY_LEN - FRAME_LEN..])?;
    }
    if reply[..4] != REPLY_MARKER || reply[8] != code {
        return Err(Error::BadReply { code });
    }
    Ok(reply)
}

/// 101, 110, 112, then 131-138: the serial and the decoded device information.
fn query_pass<T: Read + Write>(
    transport: &mut T,
) -> Result<([u8; SERIAL_PAIRS * 2], DeviceInfo), Error> {
    let reply = request(transport, REQUEST_SERIAL)?;
    let mut serial = [0; SERIAL_PAIRS * 2];
    for (i, pair) in serial.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let at = SERIAL_START + i * 3;
        pair.copy_from_slice(&reply[at..at + 2]);
    }
    let system1 = request(transport, REQUEST_SYSTEM_INFO_1)?;
    let system2 = request(transport, REQUEST_SYSTEM_INFO_2)?;
    let mut channels = [[0; REPLY_LEN]; 8];
    for (slot, code) in channels.iter_mut().zip(REQUEST_CHANNEL_INFO) {
        *slot = request(transport, code)?;
    }
    Ok((serial, DeviceInfo::parse(&system1, &system2, &channels)))
}
