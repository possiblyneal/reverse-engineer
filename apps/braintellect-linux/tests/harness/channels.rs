//! The clone's decode of a stream, per channel: the whole connect -> handshake ->
//! start -> stream flow over the in-memory transport, collected as microvolts
//! `[channel][sample]` for [`super::assertions::channel_microvolts`].

use braintellect_linux::acquisition::decode::ChannelCalibration;
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::acquisition::stream::{StreamConfig, StreamStats};

use super::fixtures;
use super::transport::ReplayTransport;

/// Decode `stream` (chunks as the link delivered them) at the baseline calibration; the
/// session's final error counters come with it.
pub fn decode(stream: Vec<Vec<u8>>) -> (Vec<Vec<f64>>, StreamStats) {
    let record = fixtures::stored_record();
    let config = StreamConfig {
        calibration: ChannelCalibration::new(
            record.number("header.deviceGain"),
            record.number("header.sensorGain"),
        )
        .expect("baseline calibration is valid"),
        family: None,
    };
    let mut session =
        Session::open(ReplayTransport::new(fixtures::wire(), stream), config).expect("handshake");
    session.start(4).expect("start");
    let mut channels = vec![Vec::new(); 4];
    while let Some(frame) = session.next_frame().expect("stream") {
        for (out, samples) in channels.iter_mut().zip(frame.channels) {
            out.extend(samples);
        }
    }
    (channels, session.stats())
}
