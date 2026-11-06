//! Command encoding: `!*#<code>,<a1>,<a2>,<a3>,<a4>,<a5>,<a6>\r\n`.
//!
//! Wire codes and argument positions are the ones observed in the 2026-11-04
//! traced session (`b2-protocol.md`, "Commands sent"). The clone never sends
//! power-off (a seven-argument frame); it is encoded and decoded for the B-2
//! emulator, which has to recognise it. The other parametric setters are not
//! implemented.

/// Handshake request: serial number.
pub const REQUEST_SERIAL: u8 = 101;
/// Handshake request: system info block 1.
pub const REQUEST_SYSTEM_INFO_1: u8 = 110;
/// Handshake request: system info block 2.
pub const REQUEST_SYSTEM_INFO_2: u8 = 112;
/// Channel information requests, one per hardware channel (131-138).
pub const REQUEST_CHANNEL_INFO: std::ops::RangeInclusive<u8> = 131..=138;

/// Whether `code` is a query the device answers with one 56-byte reply.
#[must_use]
pub fn is_request(code: u8) -> bool {
    matches!(
        code,
        REQUEST_SERIAL | REQUEST_SYSTEM_INFO_1 | REQUEST_SYSTEM_INFO_2
    ) || REQUEST_CHANNEL_INFO.contains(&code)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// A query (101, 110, 112, 131-138); the device answers with one 56-byte reply.
    Request(u8),
    /// `setStop`.
    Stop,
    /// `setMode(Normal EEG)`.
    SetModeNormalEeg,
    /// `setSignalInjection(OFF)`.
    SetSignalInjectionOff,
    /// `setStart(delay 0, <channels> channels)`: code 48 with a1 = 1 and a3 =
    /// channel count (a2 = delay is inferred from the delay 0 / 4 channels run).
    Start { channels: u8 },
    /// `setPower(OFF)`: code 48 with a2 = 1, seven arguments, sent at app exit.
    PowerOff,
}

impl Command {
    /// The bytes written to the controller.
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        let (code, args): (u8, &[u8]) = match self {
            Self::Request(code) => (code, &[0; 6]),
            Self::Stop => (48, &[0; 6]),
            Self::SetModeNormalEeg => (40, &[1, 0, 0, 0, 0, 0]),
            Self::SetSignalInjectionOff => (46, &[0; 6]),
            Self::Start { channels } => (48, &[1, 0, channels, 0, 0, 0]),
            Self::PowerOff => (48, &[0, 1, 0, 0, 0, 0, 0]),
        };
        let mut text = format!("!*#{code}");
        for arg in args {
            text.push_str(&format!(",{arg}"));
        }
        text.push_str("\r\n");
        text.into_bytes()
    }

    /// The command a line of wire bytes (with or without its `\r\n`) encodes, or
    /// `None` for a line this module does not know.
    #[must_use]
    pub fn decode(line: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(line)
            .ok()?
            .trim_end_matches(['\r', '\n']);
        let mut fields = text.strip_prefix("!*#")?.split(',');
        let code: u8 = fields.next()?.parse().ok()?;
        let args: Vec<u8> = fields.map(str::parse).collect::<Result<_, _>>().ok()?;
        match (code, args.as_slice()) {
            (48, [0, 0, 0, 0, 0, 0]) => Some(Self::Stop),
            (48, [1, 0, channels, 0, 0, 0]) => Some(Self::Start {
                channels: *channels,
            }),
            (48, [0, 1, 0, 0, 0, 0, 0]) => Some(Self::PowerOff),
            (40, [1, 0, 0, 0, 0, 0]) => Some(Self::SetModeNormalEeg),
            (46, [0, 0, 0, 0, 0, 0]) => Some(Self::SetSignalInjectionOff),
            (_, [0, 0, 0, 0, 0, 0]) if is_request(code) => Some(Self::Request(code)),
            _ => None,
        }
    }
}
