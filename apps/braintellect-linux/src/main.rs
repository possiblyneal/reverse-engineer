//! Three commands.
//!
//! `braintellect-linux ui --mac <MAC>` opens the session window: connection and
//! signal status, session properties, start and stop, live feedback, and the
//! one-time instrument import. This is the clinician's entry point.
//!
//! `braintellect-linux import <lib-dir>` copies the original's instrument banks
//! (`dll<id>.BT.dll`) from its `lib` directory into the clone's data directory.
//!
//! `braintellect-linux --mac <MAC>` runs live acquisition against a real B-2.
//! It connects over RFCOMM, runs the handshake, starts streaming, and prints one
//! line of microvolts per frame until the stream ends or Ctrl-C. Needs a paired
//! adapter that is not owned by another host; CI never runs it.

use std::path::Path;
use std::process::ExitCode;

use braintellect_linux::acquisition::rfcomm::{RfcommStream, DEFAULT_CHANNEL};
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::audio::import;
use braintellect_linux::live;
use braintellect_linux::ui;

const ACTIVE_CHANNELS: u8 = 4;

fn run(mac: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::open(
        RfcommStream::connect(mac, DEFAULT_CHANNEL)?,
        live::stream_config()?,
    )?;
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

const USAGE: &str = "usage: braintellect-linux ui --mac <AA:BB:CC:DD:EE:FF>\n       braintellect-linux --mac <AA:BB:CC:DD:EE:FF>\n       braintellect-linux import <original lib dir>";

fn import_banks(lib_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dest = import::default_dir().ok_or("neither XDG_DATA_HOME nor HOME is set")?;
    for path in import::import_banks(Path::new(lib_dir), &dest)? {
        println!("{}", path.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["ui", "--mac", mac] => ui::app::run((*mac).to_owned()).map_err(Into::into),
        ["--mac", mac] => run(mac),
        ["import", lib_dir] => import_banks(lib_dir),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
