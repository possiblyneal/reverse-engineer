//! Live acquisition against a real B-2: `braintellect-linux --mac <MAC>`.
//!
//! Connects over RFCOMM, runs the handshake, starts streaming, and prints one
//! line of microvolts per frame until the stream ends or Ctrl-C. Needs a paired
//! adapter that is not owned by another host; CI never runs it.

use std::process::ExitCode;

use braintellect_linux::acquisition::decode::{ChannelCalibration, DeviceFamily};
use braintellect_linux::acquisition::rfcomm::{RfcommStream, DEFAULT_CHANNEL};
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::acquisition::stream::StreamConfig;

/// Baseline calibration (`b2-protocol.md`, "Calibration confirmed"): device
/// family 12, channelGain 12, sensorGain 10. Not read from the device.
const FAMILY: DeviceFamily = DeviceFamily::F12;
const CHANNEL_GAIN: f64 = 12.0;
const SENSOR_GAIN: f64 = 10.0;
const ACTIVE_CHANNELS: u8 = 4;

fn run(mac: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = StreamConfig {
        calibration: ChannelCalibration::new(CHANNEL_GAIN, SENSOR_GAIN)
            .map_err(|e| format!("{e:?}"))?,
        family: FAMILY,
    };
    let mut session = Session::open(RfcommStream::connect(mac, DEFAULT_CHANNEL)?, config)?;
    eprintln!(
        "connected, serial {}",
        String::from_utf8_lossy(session.serial())
    );
    session.start(ACTIVE_CHANNELS)?;
    while let Some(frame) = session.next_frame()? {
        let uv: Vec<String> = frame
            .channels
            .iter()
            .flatten()
            .map(|v| format!("{v:.3}"))
            .collect();
        println!("{}\t{}", frame.counter, uv.join("\t"));
    }
    // The device closed the stream: there is no link left to send Stop on.
    eprintln!("stream ended, {:?}", session.stats());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, flag, mac] = args.as_slice() else {
        eprintln!("usage: braintellect-linux --mac <AA:BB:CC:DD:EE:FF>");
        return ExitCode::from(2);
    };
    if flag != "--mac" {
        eprintln!("usage: braintellect-linux --mac <AA:BB:CC:DD:EE:FF>");
        return ExitCode::from(2);
    }
    match run(mac) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
