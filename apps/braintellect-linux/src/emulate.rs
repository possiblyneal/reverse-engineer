//! A B-2 emulator: serves a recorded raw RX capture over a serial endpoint so
//! the original app can be run against the stream the clone (or the original)
//! once received. It speaks the protocol of `b2-protocol.md` from the device's
//! side: each `!*#` request is answered with the next recorded reply for that
//! code, Start begins the recorded frames at the recorded rate, Stop and
//! power-off end them. Nothing the B-2 did not send is ever sent, so the
//! setters (mode, signal injection) and power-off get no reply.
//!
//! The capture is the raw bytes a client read from the link
//! ([`crate::capture`]): handshake replies and stream frames, found by their
//! marker. Protocol logic runs over `Read + Write`, so tests and the Unix
//! socket the CLI opens are the same code.

use std::io::{self, ErrorKind, Read, Write};

use crate::acquisition::command::{self, Command};
use crate::acquisition::session::REPLY_LEN;
use crate::acquisition::stream::{FRAME_LEN, MARKER};

/// Frames a second: 500 Hz per channel, two samples per frame (`b2-protocol.md`).
const FRAMES_PER_SECOND: u64 = 250;
/// Replies echo their request code at `[8]` (`b2-protocol.md`, reply layout).
const CODE_AT: usize = 8;

/// A capture split into what the device sent: replies by request code, in the
/// order they were sent, and the stream frames in order.
#[derive(Debug, Default)]
pub struct Capture {
    replies: Vec<(u8, Vec<[u8; REPLY_LEN]>)>,
    frames: Vec<[u8; FRAME_LEN]>,
}

impl Capture {
    /// Split raw RX bytes. A marker followed by another marker 32 bytes on is a
    /// stream frame; one whose `[8]` is a request code and is not is a 56-byte
    /// reply. Bytes between them (a partial read at the start) and a cut-off
    /// last item are dropped. Frames before the last reply were streaming before the
    /// handshake (a B-2 left running), which the session skips, so they are dropped too.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        let mut capture = Self::default();
        let mut at = 0;
        while let Some(found) = find(bytes, &MARKER, at) {
            let rest = &bytes[found..];
            let is_frame = rest.len() >= FRAME_LEN + MARKER.len()
                && rest[FRAME_LEN..FRAME_LEN + MARKER.len()] == MARKER;
            // The last frame, followed by nothing or by a cut-off marker.
            let is_last_frame = (FRAME_LEN..FRAME_LEN + MARKER.len()).contains(&rest.len())
                && MARKER.starts_with(&rest[FRAME_LEN..])
                && rest[6..8] == [3, 3];
            if is_frame || is_last_frame {
                capture
                    .frames
                    .push(rest[..FRAME_LEN].try_into().expect("32"));
                at = found + FRAME_LEN;
            } else if rest.len() >= REPLY_LEN && command::is_request(rest[CODE_AT]) {
                let reply: [u8; REPLY_LEN] = rest[..REPLY_LEN].try_into().expect("56");
                capture.push_reply(reply);
                capture.frames.clear();
                at = found + REPLY_LEN;
            } else {
                at = found + 1;
            }
        }
        capture
    }

    fn push_reply(&mut self, reply: [u8; REPLY_LEN]) {
        let code = reply[CODE_AT];
        match self.replies.iter_mut().find(|(c, _)| *c == code) {
            Some((_, list)) => list.push(reply),
            None => self.replies.push((code, vec![reply])),
        }
    }
}

fn find(bytes: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// The device's state while a client talks to it.
struct Emulator<'a> {
    capture: &'a Capture,
    /// Requests answered so far, per code: the next one gets the next reply.
    answered: Vec<(u8, usize)>,
    /// Command bytes not yet ended by a newline.
    pending: Vec<u8>,
    /// Frames sent so far; a restart carries on from here.
    position: usize,
    /// While streaming: when Start arrived and how many frames it has sent.
    run: Option<Run>,
}

struct Run {
    started_ms: u64,
    sent: u64,
}

impl<'a> Emulator<'a> {
    fn new(capture: &'a Capture) -> Self {
        Self {
            capture,
            answered: Vec::new(),
            pending: Vec::new(),
            position: 0,
            run: None,
        }
    }

    /// Feed bytes the client wrote at `now_ms`; returns the replies to send now.
    fn input(&mut self, bytes: &[u8], now_ms: u64) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            match Command::decode(&line) {
                Some(Command::Request(code)) => out.extend(self.reply(code)),
                Some(Command::Start { .. }) => {
                    self.run.get_or_insert(Run {
                        started_ms: now_ms,
                        sent: 0,
                    });
                }
                Some(Command::Stop | Command::PowerOff) => self.run = None,
                _ => {}
            }
        }
        out
    }

    /// The frames the stream owes at `now_ms`: 250 a second since Start, until
    /// the capture runs out.
    fn due(&mut self, now_ms: u64) -> Vec<u8> {
        let Some(run) = &mut self.run else {
            return Vec::new();
        };
        let owed = now_ms.saturating_sub(run.started_ms) * FRAMES_PER_SECOND / 1000;
        let count = usize::try_from(owed - run.sent).unwrap_or(usize::MAX);
        let frames = &self.capture.frames[self.position.min(self.capture.frames.len())..];
        let frames = &frames[..count.min(frames.len())];
        run.sent = owed;
        self.position += frames.len();
        frames.concat()
    }

    /// The next recorded reply to `code`, the last one again once they run out.
    fn reply(&mut self, code: u8) -> Vec<u8> {
        let Some((_, list)) = self.capture.replies.iter().find(|(c, _)| *c == code) else {
            return Vec::new();
        };
        let n = match self.answered.iter_mut().find(|(c, _)| *c == code) {
            Some((_, n)) => n,
            None => {
                self.answered.push((code, 0));
                &mut self.answered.last_mut().expect("just pushed").1
            }
        };
        let reply = list[(*n).min(list.len() - 1)];
        *n += 1;
        reply.to_vec()
    }
}

/// Serve `capture` to the client on `link` until it closes the link. `clock`
/// reads milliseconds on a monotonic clock, once per turn; `link` should time
/// reads out (`TimedOut`/`WouldBlock`) often enough to pace the stream.
///
/// # Errors
/// Fails on a link error other than a read timeout.
pub fn serve<T: Read + Write>(
    link: &mut T,
    capture: &Capture,
    clock: &mut dyn FnMut() -> u64,
) -> io::Result<()> {
    let mut emulator = Emulator::new(capture);
    let mut buf = [0u8; 256];
    loop {
        let n = match link.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => 0,
            Err(e) if e.kind() == ErrorKind::Interrupted => 0,
            Err(e) => return Err(e),
        };
        let now = clock();
        let mut out = emulator.input(&buf[..n], now);
        out.extend(emulator.due(now));
        if !out.is_empty() {
            link.write_all(&out)?;
            link.flush()?;
        }
    }
}
