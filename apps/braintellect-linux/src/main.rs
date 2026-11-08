//! Five commands.
//!
//! `braintellect-linux ui [--mac <MAC>]` opens the session window: pairing and
//! connecting to the B-2 (`--mac` names it; without it the window uses the B-2
//! BlueZ has paired or offers Pair and connect), connection and signal status, session properties, start and stop, live feedback, and the
//! one-time instrument import. This is the clinician's entry point.
//!
//! `braintellect-linux import <lib-dir>` copies the original's instrument banks
//! (`dll<id>.BT.dll`) from its `lib` directory into the clone's data directory.
//!
//! `braintellect-linux import-soundfont <file.sf2>` renders the harp, acoustic guitar
//! and xylophone tones from a General MIDI SoundFont into the same directory.
//!
//! `braintellect-linux emulate <capture> <socket>` plays a B-2 for the original app:
//! it connects to the Unix socket a QEMU serial port is backed by and serves a raw
//! capture recorded by a live session (`capture.rs`, `emulate.rs`).
//!
//! `braintellect-linux --mac <MAC>` runs live acquisition against a real B-2.
//! It connects over RFCOMM, runs the handshake, starts streaming, and prints one
//! line of microvolts per frame until the stream ends or Ctrl-C. Needs a paired
//! adapter that is not owned by another host; CI never runs it.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use braintellect_linux::acquisition::rfcomm::{RfcommStream, DEFAULT_CHANNEL};
use braintellect_linux::acquisition::session::Session;
use braintellect_linux::audio::{import, tone};
use braintellect_linux::emulate::{self, Capture};
use braintellect_linux::live;
use braintellect_linux::storage;
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

const USAGE: &str = "usage: braintellect-linux ui [--mac <AA:BB:CC:DD:EE:FF>]\n       braintellect-linux --mac <AA:BB:CC:DD:EE:FF>\n       braintellect-linux import <original lib dir>\n       braintellect-linux import-soundfont <file.sf2>\n       braintellect-linux emulate <capture file> <serial socket>";

/// How long the emulator waits for the app's bytes before it checks the stream
/// clock; a frame is due every 4 ms.
const EMULATE_TICK: Duration = Duration::from_millis(1);

/// The emulator's link, echoing what the app writes to stderr with the time it
/// arrived, so a rig run can be lined up against the app's own log.
struct Traced(UnixStream);

impl Read for Traced {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.0.read(buf)?;
        if n > 0 {
            let at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            eprintln!("{at} app> {}", buf[..n].escape_ascii());
        }
        Ok(n)
    }
}

impl Write for Traced {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

fn emulate_capture(capture: &str, socket: &str) -> Result<(), Box<dyn std::error::Error>> {
    let capture = Capture::parse(&std::fs::read(capture)?);
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(EMULATE_TICK))?;
    let mut link = Traced(stream);
    eprintln!("emulating the B-2 on {socket}");
    let origin = Instant::now();
    emulate::serve(&mut link, &capture, &mut || {
        u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    })?;
    Ok(())
}

fn import_banks(lib_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dest = import::default_dir().ok_or(storage::NO_DATA_DIR)?;
    for path in import::import_banks(Path::new(lib_dir), &dest)? {
        println!("{}", path.display());
    }
    Ok(())
}

fn import_soundfont(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dest = import::default_dir().ok_or(storage::NO_DATA_DIR)?;
    for path in tone::import_soundfont(&std::fs::read(file)?, &dest)? {
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
        ["ui"] => ui::app::run(None).map_err(Into::into),
        ["ui", "--mac", mac] => ui::app::run(Some((*mac).to_owned())).map_err(Into::into),
        ["--mac", mac] => run(mac),
        ["import", lib_dir] => import_banks(lib_dir),
        ["import-soundfont", file] => import_soundfont(file),
        ["emulate", capture, socket] => emulate_capture(capture, socket),
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
