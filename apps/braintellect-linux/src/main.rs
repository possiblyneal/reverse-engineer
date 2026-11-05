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

/// Baseline calibration: family 12, channelGain 12, sensorGain 10
/// (`b2-protocol.md`, "Calibration confirmed"). Not read from the device.
const ACTIVE_CHANNELS: u8 = 4;

fn run(mac: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = StreamConfig {
        calibration: ChannelCalibration::new(12.0, 10.0).map_err(|e| format!("{e:?}"))?,
        family: DeviceFamily::F12,
    };
    let mut session = Session::open(RfcommStream::connect(mac, DEFAULT_CHANNEL)?, config)?;
    eprintln!(
        "connected, serial {}",
        String::from_utf8_lossy(session.serial())
    );
    session.start(ACTIVE_CHANNELS)?;
    while let Some(frame) = session.next_frame()? {
        let uv: Vec<String> = frame.microvolts.iter().map(|v| format!("{v:.3}")).collect();
        println!("{}\t{}", frame.counter, uv.join("\t"));
    }
    eprintln!("stream ended, {:?}", session.stats());
    session.disconnect()?;
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
