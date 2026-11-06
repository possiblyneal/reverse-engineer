//! The inbound sample stream: 32-byte frames, resynchronised on the marker.
//!
//! Frame layout, counter and `sampleValidation` rule are from
//! `b2-protocol.md` ("Observed stream frame (COM-handle capture)" and
//! "sampleValidation and counter errors"). The static `processData_v5` reading
//! (0x64 sync, 4-byte channel groups) is unconfirmed legacy and not used.
//!
//! Channel `c` (0..3) owns payload slots `c` and `4 + c`, oldest first
//! (`b2-protocol.md`, "Unresolved", observed in run 4), so a frame surfaces as 4
//! channels of 2 samples each.

use super::decode::{decode_sample_i24_be, ChannelCalibration, DeviceFamily};

/// Frame marker (`AC BC CC 03`).
pub const MARKER: [u8; 4] = [0xAC, 0xBC, 0xCC, 0x03];
/// Marker, 16-bit counter, `03 03`, then the payload; no trailer.
pub const FRAME_LEN: usize = 32;
const PAYLOAD_START: usize = 8;
/// Samples per frame: 8 x 24-bit big-endian signed.
pub const SAMPLES_PER_FRAME: usize = 8;
/// Active channels the payload carries.
pub const CHANNELS: usize = 4;
/// Samples of each channel in one frame.
pub const SAMPLES_PER_CHANNEL: usize = SAMPLES_PER_FRAME / CHANNELS;

/// What converts raw samples to microvolts. The device family and channel gain
/// are not decoded from the handshake (device-vs-app family is unresolved), so
/// the caller supplies them.
#[derive(Debug, Clone, Copy)]
pub struct StreamConfig {
    pub calibration: ChannelCalibration,
    pub family: DeviceFamily,
}

/// One validated frame, decoded: the frame counter and each channel's two
/// samples (oldest first), in microvolts. Indexed `[channel][sample]`.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleFrame {
    pub counter: u16,
    pub channels: [[f64; SAMPLES_PER_CHANNEL]; CHANNELS],
}

/// The error counters the original keeps (`DataBuffer.packetCounterErrors` and
/// `sampleValidationErrors`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamStats {
    /// Frames whose counter was not previous + 1 (mod 65536).
    pub packet_counter_errors: u32,
    /// Frames failing `sampleValidation`: a header `03 03` byte wrong, or any
    /// sample's third byte without its low two bits set (`& 3 == 3`).
    pub sample_validation_errors: u32,
}

/// Incremental frame parser: feed arbitrary byte runs, take frames out.
#[derive(Debug, Default)]
pub struct FrameParser {
    buf: Vec<u8>,
    last_counter: Option<u16>,
    stats: StreamStats,
}

impl FrameParser {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    #[must_use]
    pub fn stats(&self) -> StreamStats {
        self.stats
    }

    /// The next valid frame, or `None` when more bytes are needed. Checks run in
    /// the original's order (`b2-protocol.md`, "sampleValidation and counter
    /// errors"): a frame with a wrong `03 03` header is counted and skipped; otherwise
    /// its counter is checked and adopted (`0x55a549`) before the sample bits
    /// (`0x55a69c`), so a frame failing only the sample check is counted and
    /// dropped but its counter still becomes the reference. A frame whose counter
    /// is off is counted and still delivered.
    pub fn next_frame(&mut self) -> Option<(u16, [u8; 24])> {
        loop {
            let Some(start) = self.buf.windows(MARKER.len()).position(|w| w == MARKER) else {
                // Keep a possible marker prefix; everything before it is noise.
                let keep = self.buf.len().min(MARKER.len() - 1);
                self.buf.drain(..self.buf.len() - keep);
                return None;
            };
            self.buf.drain(..start);
            if self.buf.len() < FRAME_LEN {
                return None;
            }
            let frame = &self.buf[..FRAME_LEN];
            if frame[6] != 3 || frame[7] != 3 {
                self.stats.sample_validation_errors += 1;
                self.buf.drain(..MARKER.len());
                continue;
            }
            let counter = u16::from_be_bytes([frame[4], frame[5]]);
            if self
                .last_counter
                .is_some_and(|p| counter != p.wrapping_add(1))
            {
                self.stats.packet_counter_errors += 1;
            }
            self.last_counter = Some(counter);
            let payload: [u8; 24] = frame[PAYLOAD_START..].try_into().expect("24-byte payload");
            if !payload.as_chunks::<3>().0.iter().all(|s| s[2] & 3 == 3) {
                self.stats.sample_validation_errors += 1;
                self.buf.drain(..FRAME_LEN);
                continue;
            }
            self.buf.drain(..FRAME_LEN);
            return Some((counter, payload));
        }
    }
}

/// Decode a payload's 8 samples to microvolts (`b2-protocol.md`, ADC -> volts),
/// regrouped by channel: channel `c` takes payload slots `c` and `4 + c`.
#[must_use]
pub fn decode_payload(
    payload: &[u8; 24],
    config: StreamConfig,
) -> [[f64; SAMPLES_PER_CHANNEL]; CHANNELS] {
    let slot = |i: usize| {
        let s = &payload[3 * i..3 * i + 3];
        let raw = decode_sample_i24_be([s[0], s[1], s[2]]);
        config.calibration.to_volts(raw, config.family) * 1e6
    };
    std::array::from_fn(|c| std::array::from_fn(|n| slot(c + CHANNELS * n)))
}
