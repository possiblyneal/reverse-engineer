//! Runs one session on its own thread so the window never blocks on the link or
//! the audio device. This is the glue to the real devices (RFCOMM, ALSA, the
//! user's data directory); CI never runs it, and the engine it drives is tested
//! in `tests/live_session.rs`.

use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::acquisition::rfcomm::{RfcommStream, DEFAULT_CHANNEL};
use crate::acquisition::session::Session;
use crate::audio::import;
use crate::audio::sink::AlsaSink;
use crate::audio::tone::{self, Tone};
use crate::capture::{self, Recording};
use crate::live::{self, Environment, GraphStart, SessionProperties, OUTPUT_RATE_HZ};
use crate::storage::{self, Store};

use super::bluetooth::{answered, connect_b2, OpenError, PairFailure};
use super::bluez::Bluez;
use super::state::Event;

/// How long a read waits on a quiet link before the engine looks at Stop.
const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// A session running on its own thread.
pub struct Worker {
    stop: Arc<AtomicBool>,
    pub events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Connect to the B-2 at `mac` and run a session playing `tone`; `wake` is
    /// called after each event so the window redraws.
    pub fn spawn(
        mac: String,
        properties: SessionProperties,
        tone: &'static Tone,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        Self::spawn_job(wake, move |flag, send| {
            session(&mac, &properties, tone, flag, send)
        })
    }

    /// Connect to the B-2 at `mac` and report the sensor status until stopped; the
    /// events end with [`Event::SensorCheckEnded`].
    pub fn spawn_check(
        mac: String,
        properties: SessionProperties,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        Self::spawn_job(wake, move |flag, send| {
            let result = check(&mac, &properties, flag, send);
            send(Event::SensorCheckEnded(result.err()));
            Ok(())
        })
    }

    /// Find the B-2, pair and trust it, then report its sensor status until stopped.
    /// The events are [`Event::Paired`] or [`Event::PairFailed`], then those of
    /// [`Self::spawn_check`].
    pub fn spawn_pair_and_connect(
        properties: SessionProperties,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        Self::spawn_job(wake, move |flag, send| {
            match open_b2(None) {
                Ok((mac, stream)) => {
                    send(Event::Paired(mac));
                    let result = monitor(stream, &properties, flag, send);
                    send(Event::SensorCheckEnded(result.err()));
                }
                Err(failure) => send(Event::PairFailed(failure)),
            }
            Ok(())
        })
    }

    /// Play a short preview of `tone` on the audio output; a failure arrives as
    /// [`Event::PreviewFailed`].
    pub fn spawn_preview(tone: &'static Tone, wake: impl Fn() + Send + 'static) -> Self {
        Self::spawn_job(wake, move |_, send| {
            if let Err(why) = preview(tone) {
                send(Event::PreviewFailed(why));
            }
            Ok(())
        })
    }

    /// Try to reach the B-2 at `mac` and report [`Event::Presence`]. A B-2 that
    /// answers keeps the link until it drops, which reports it off at once, or until
    /// [`Self::stop`] frees it for a session. A page blocks up to its timeout.
    pub fn spawn_probe(mac: String, wake: impl Fn() + Send + 'static) -> Self {
        Self::spawn_job(wake, move |stop, send| {
            match RfcommStream::connect(&mac, DEFAULT_CHANNEL) {
                Ok(stream) => {
                    send(Event::Presence(true));
                    if !hold(stream, stop) {
                        send(Event::Presence(false));
                    }
                }
                Err(e) => send(Event::Presence(answered(&e))),
            }
            Ok(())
        })
    }

    fn spawn_job(
        wake: impl Fn() + Send + 'static,
        job: impl FnOnce(&AtomicBool, &dyn Fn(Event)) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (send, events) = channel();
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let send = |event: Event| {
                // The window closing drops the receiver; nothing is left to tell.
                let _ = send.send(event);
                wake();
            };
            if let Err(why) = job(&flag, &send) {
                send(Event::Failed(why));
            }
        });
        Self {
            stop,
            events,
            thread: Some(thread),
        }
    }

    /// Whether the job's thread has ended.
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Ask the session to end; it is stored like any other end.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Closing the window mid-session stops the session and waits for it to be stored.
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open the RFCOMM link to the B-2 (at `known`, else found and paired). A refused or
/// dropped connect, or a missing link key, looks like a stale bond, so BlueZ removes
/// it and pairs again once (`bluetooth::connect_b2`). A refusal can also be a B-2
/// whose link another host holds; the bond lost then is recovered by Pair and connect.
fn open_b2(known: Option<&str>) -> Result<(String, RfcommStream), PairFailure> {
    let mut host = Bluez::connect().map_err(PairFailure::Unavailable)?;
    connect_b2(&mut host, known, |mac| {
        let stream = RfcommStream::connect(mac, DEFAULT_CHANNEL).map_err(|e| {
            let why = format!("cannot connect to {mac}: {e}");
            // The kernel reports HCI "PIN or Key Missing" as `EBADE`.
            let key_missing = e.raw_os_error() == Some(libc::EBADE);
            match e.kind() {
                ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::PermissionDenied => OpenError::Authentication(why),
                _ if key_missing => OpenError::Authentication(why),
                _ => OpenError::Other(why),
            }
        })?;
        stream
            .set_read_timeout(READ_TIMEOUT)
            .map_err(|e| OpenError::Other(e.to_string()))?;
        Ok(stream)
    })
}

/// The sensor status before a session: no audio, no store, no capture file.
fn check(
    mac: &str,
    properties: &SessionProperties,
    stop: &AtomicBool,
    send: &dyn Fn(Event),
) -> Result<(), String> {
    let Some(stream) = open_paired(mac, send)? else {
        return Ok(());
    };
    monitor(stream, properties, stop, send)
}

/// Open the link to the B-2 at `mac`. A lost bond is told as [`Event::PairFailed`],
/// so the window forgets the address and offers Pair and connect, and gives `None`.
fn open_paired(mac: &str, send: &dyn Fn(Event)) -> Result<Option<RfcommStream>, String> {
    match open_b2(Some(mac)) {
        Ok((_, stream)) => Ok(Some(stream)),
        Err(failure @ PairFailure::BondLost(_)) => {
            send(Event::PairFailed(failure));
            Ok(None)
        }
        Err(failure) => Err(failure.message()),
    }
}

fn monitor(
    stream: RfcommStream,
    properties: &SessionProperties,
    stop: &AtomicBool,
    send: &dyn Fn(Event),
) -> Result<(), String> {
    let session = Session::open(stream, live::stream_config()?).map_err(|e| e.to_string())?;
    let origin = Instant::now();
    let mut clock = move || i64::try_from(origin.elapsed().as_millis()).unwrap_or(i64::MAX);
    live::monitor(session, properties, stop, &mut clock, &mut |report| {
        if let Some(fit) = report.fit {
            send(Event::Sensor(fit));
        }
        if let Some(percent) = report.lost_percent {
            send(Event::LinkLoss(percent));
        }
    })
}

/// Keep an idle link open until `stop` is set (`true`) or the link drops (`false`).
/// The B-2 sends nothing before Start, so any read that does not time out ends it.
fn hold(mut stream: RfcommStream, stop: &AtomicBool) -> bool {
    if stream.set_read_timeout(READ_TIMEOUT).is_err() {
        return false;
    }
    let mut byte = [0u8; 1];
    while !stop.load(Ordering::Relaxed) {
        match stream.read(&mut byte) {
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Ok(n) if n > 0 => {}
            _ => return false,
        }
    }
    true
}

fn preview(tone: &Tone) -> Result<(), String> {
    let dir = import::default_dir().ok_or(storage::NO_DATA_DIR)?;
    let bank = tone::load_tone(&dir, tone).map_err(|e| e.to_string())?;
    let pcm = tone::preview(&bank).map_err(|e| e.to_string())?;
    let mut sink = AlsaSink::open(OUTPUT_RATE_HZ).map_err(|e| format!("no audio output: {e}"))?;
    sink.write(&pcm).map_err(|e| e.to_string())
}

fn session(
    mac: &str,
    properties: &SessionProperties,
    tone: &Tone,
    stop: &AtomicBool,
    send: &dyn Fn(Event),
) -> Result<(), String> {
    let bank_dir = import::default_dir().ok_or(storage::NO_DATA_DIR)?;
    let bank = tone::load_tone(&bank_dir, tone).map_err(|e| e.to_string())?;
    let store = Store::open(&storage::default_path().ok_or(storage::NO_DATA_DIR)?)
        .map_err(|e| e.to_string())?;
    let mut sink = AlsaSink::open(OUTPUT_RATE_HZ).map_err(|e| format!("no audio output: {e}"))?;
    let Some(stream) = open_paired(mac, send)? else {
        return Ok(());
    };
    // The raw RX bytes, handshake included, go to the data directory: the B-2
    // emulator replays them into the original app (`capture.rs`).
    let data_dir = storage::default_data_dir().ok_or(storage::NO_DATA_DIR)?;
    let (file, path) = capture::create(&data_dir).map_err(|e| format!("no capture file: {e}"))?;
    eprintln!("recording the raw stream to {}", path.display());
    let capture = path
        .strip_prefix(&data_dir)
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned();
    let session = Session::open(Recording::new(stream, file), live::stream_config()?)
        .map_err(|e| e.to_string())?;
    if stop.load(Ordering::Relaxed) {
        send(Event::Cancelled);
        return Ok(());
    }
    let origin = Instant::now();
    let mut clock = move || i64::try_from(origin.elapsed().as_millis()).unwrap_or(i64::MAX);
    let mut report = |snapshot: &live::Snapshot| send(Event::Update(snapshot.clone()));
    live::run(
        session,
        properties,
        Environment {
            bank: &bank,
            sink: &mut sink,
            store: &store,
            stop,
            clock: &mut clock,
            graph: GraphStart::LIVE,
            report: &mut report,
            capture: Some(capture),
        },
    );
    Ok(())
}
