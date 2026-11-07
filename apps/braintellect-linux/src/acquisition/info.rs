//! Decoded handshake replies: 110, 112 and 131-138.
//!
//! Every reply is 56 bytes: `AC BC CC 03`, `03 03 03 03`, the echoed request code at
//! `[8]`, `03`, then two-byte pairs each followed by a `03` separator from `[10]`
//! (`b2-protocol.md`, "Reply layout"). Which pair bytes the app reads is the static
//! reading of its device initialisation (`FUN_00552540`, which copies them out of the
//! 110 and 112 replies, and `initializeDevice`, VA `0x55b850`, which passes the family
//! byte to `setDeviceFamily`, VA `0x55bad8`-`0x55bae3`). Bytes the app never reads are
//! not decoded: the 110 pair 1 first byte and pair 2 second byte, the 112 payload
//! (kept raw in [`DeviceInfo::system2`]) and the 110 pair 4 second byte.

use super::decode::DeviceFamily;
use super::session::REPLY_LEN;

/// Payload start: pair `i` is `reply[10 + 3i]`, `reply[11 + 3i]`.
const PAYLOAD_START: usize = 10;

fn pair(reply: &[u8; REPLY_LEN], i: usize) -> [u8; 2] {
    let at = PAYLOAD_START + 3 * i;
    [reply[at], reply[at + 1]]
}

/// The 110 reply (`SYSTEM_INFO_1`), as the app logs it at device initialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemInfo1 {
    /// The device family code the app hands to `setDeviceFamily`: pair 1, second byte.
    pub family: u8,
    /// `numberChannels (from device)`: pair 2, first byte.
    pub channels: u8,
    /// `FW version`: pair 3 first and second byte, pair 4 first byte.
    pub firmware: [u8; 3],
    /// `model` (`0002.0004`): pairs 5 and 6 as big-endian 16-bit numbers.
    pub model: [u16; 2],
}

impl SystemInfo1 {
    /// Decode a 110 reply.
    #[must_use]
    pub fn parse(reply: &[u8; REPLY_LEN]) -> Self {
        Self {
            family: pair(reply, 0)[1],
            channels: pair(reply, 1)[0],
            firmware: [pair(reply, 2)[0], pair(reply, 2)[1], pair(reply, 3)[0]],
            model: [
                u16::from_be_bytes(pair(reply, 4)),
                u16::from_be_bytes(pair(reply, 5)),
            ],
        }
    }
}

/// One hardware channel's 131-138 reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelInfo {
    /// Channel number, 1 to 8: pair 1, first byte.
    pub number: u8,
    /// Pair 1, second byte. Inferred: the device's current mode (2 when switched on,
    /// 1 after `setMode(Normal EEG)`, 0 for an inactive channel).
    pub mode: u8,
    /// Pair 2, second byte. Inferred: the channel gain (12 on the four active
    /// channels, matching `channelGain` in the app's `dumpDebug`).
    pub gain: u8,
}

impl ChannelInfo {
    /// Decode a 131-138 reply.
    #[must_use]
    pub fn parse(reply: &[u8; REPLY_LEN]) -> Self {
        let [number, mode] = pair(reply, 0);
        Self {
            number,
            mode,
            gain: pair(reply, 1)[1],
        }
    }
}

/// Everything the 110, 112 and 131-138 replies of the latest query pass say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfo {
    pub system1: SystemInfo1,
    /// The 112 reply's one payload pair (the rest of the reply is `03` filler). The
    /// app keeps it but never reads it at initialisation; its meaning is unresolved.
    pub system2: [u8; 2],
    /// Requests 131-138, in order.
    pub channels: [ChannelInfo; 8],
}

impl DeviceInfo {
    /// Decode the replies of one query pass: 110, 112, then 131-138.
    #[must_use]
    pub fn parse(
        system1: &[u8; REPLY_LEN],
        system2: &[u8; REPLY_LEN],
        channels: &[[u8; REPLY_LEN]; 8],
    ) -> Self {
        Self {
            system1: SystemInfo1::parse(system1),
            system2: pair(system2, 0),
            channels: channels.each_ref().map(ChannelInfo::parse),
        }
    }

    /// The device family the B-2 reports, or the unaccepted code (`setDeviceFamily`
    /// takes 11, 12 and 99 only).
    ///
    /// # Errors
    /// The reported code when it is none of those.
    pub fn family(&self) -> Result<DeviceFamily, u8> {
        DeviceFamily::from_code(self.system1.family)
    }
}
