//! Handshake replies (110, 112, 131-138) decoded through the parity harness: live
//! handshakes captured from the real B-2 on 2026-11-07 (`handshake_live_{a,b}.txt`)
//! are replayed through the in-memory transport and the decoded fields are asserted
//! against the values the app logs for the same device (`b2-protocol.md`,
//! "Handshake replies, live 2026-11-07").

mod harness;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::acquisition::info::{ChannelInfo, SystemInfo1};
use braintellect_linux::acquisition::session::{Error, Session};
use braintellect_linux::acquisition::stream::StreamConfig;
use harness::fixtures::{self, Dir, WireEvent};
use harness::transport::ReplayTransport;

fn config(family: Option<DeviceFamily>) -> StreamConfig {
    StreamConfig {
        calibration: ChannelCalibration::new(12.0, 10.0).expect("baseline calibration is valid"),
        family,
    }
}

fn open(
    wire: Vec<WireEvent>,
    family: Option<DeviceFamily>,
) -> Result<Session<ReplayTransport>, Error> {
    Session::open(ReplayTransport::new(wire, Vec::new()), config(family))
}

/// The 56-byte replies in the order the clone queried them: 101, 110, 112, 131-138,
/// then the same eleven again after the mode is set.
fn replies(wire: &[WireEvent]) -> Vec<[u8; 56]> {
    wire.iter()
        .filter(|e| e.dir == Dir::Rx)
        .map(|e| e.bytes.clone().try_into().expect("56-byte reply"))
        .collect()
}

/// What the app logs for this B-2: `family: 12`, `numberChannels (from device): 4`,
/// `model: 0002.0004`, `FW version: 17.1.1`.
const SYSTEM_INFO_1: SystemInfo1 = SystemInfo1 {
    family: 12,
    channels: 4,
    firmware: [17, 1, 1],
    model: [2, 4],
};

#[test]
fn live_handshakes_decode_the_system_info() {
    for tag in ['a', 'b'] {
        let session = open(fixtures::live_handshake(tag), None).expect("handshake");
        let info = session.info();
        assert_eq!(info.system1, SYSTEM_INFO_1, "handshake {tag}");
        assert_eq!(info.system2, [0x23, 0x83], "handshake {tag}");
    }
}

#[test]
fn live_handshakes_decode_the_channel_information() {
    for tag in ['a', 'b'] {
        let session = open(fixtures::live_handshake(tag), None).expect("handshake");
        let channels = session.info().channels;
        for (n, channel) in channels.iter().enumerate() {
            let number = u8::try_from(n + 1).expect("8 channels");
            let (mode, gain) = if n < 4 { (1, 12) } else { (0, 0) };
            assert_eq!(
                *channel,
                ChannelInfo { number, mode, gain },
                "handshake {tag}, request {}",
                131 + n
            );
        }
    }
}

#[test]
fn channel_mode_is_the_current_mode_not_a_constant() {
    // 131 is the fourth reply. Switched on, the B-2 reports mode 2; once the clone
    // set Normal EEG, a reconnect finds it still at 1 from the first query on.
    let first_a = ChannelInfo::parse(&replies(&fixtures::live_handshake('a'))[3]);
    let first_b = ChannelInfo::parse(&replies(&fixtures::live_handshake('b'))[3]);
    assert_eq!((first_a.number, first_a.mode), (1, 2));
    assert_eq!((first_b.number, first_b.mode), (1, 1));
}

#[test]
fn family_comes_from_the_device_unless_overridden() {
    let from_device = open(fixtures::live_handshake('a'), None).expect("handshake");
    assert_eq!(from_device.family(), DeviceFamily::F12);

    let overridden =
        open(fixtures::live_handshake('a'), Some(DeviceFamily::F99)).expect("handshake");
    assert_eq!(overridden.family(), DeviceFamily::F99);
    assert_eq!(
        overridden.info().system1.family,
        12,
        "the device still reports 12"
    );
}

#[test]
fn a_family_the_app_does_not_accept_is_an_error_unless_overridden() {
    // The app accepts 11, 12 and 99 (`setDeviceFamily`, "invalid family" otherwise).
    let mut wire = fixtures::live_handshake('a');
    for reply in wire
        .iter_mut()
        .filter(|e| e.dir == Dir::Rx && e.bytes[8] == 110)
    {
        reply.bytes[11] = 13;
    }
    let result = open(wire.clone(), None);
    assert!(matches!(result, Err(Error::UnknownFamily(13))));
    assert_eq!(
        open(wire, Some(DeviceFamily::F12))
            .expect("override")
            .family(),
        DeviceFamily::F12
    );
}
