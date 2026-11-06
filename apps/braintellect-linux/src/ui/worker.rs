//! Runs one session on its own thread so the window never blocks on the link or
//! the audio device. This is the glue to the real devices (RFCOMM, ALSA, the
//! user's data directory); CI never runs it, and the engine it drives is tested
//! in `tests/live_session.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::acquisition::rfcomm::{RfcommStream, DEFAULT_CHANNEL};
use crate::acquisition::session::Session;
use crate::audio::sink::AlsaSink;
use crate::audio::{self, import};
use crate::live::{self, Environment, SessionProperties, INSTRUMENT_ID, OUTPUT_RATE_HZ};
use crate::storage::{self, Store};

use super::state::Event;

/// How long a read waits on a quiet link before the engine looks at Stop.
const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// A session running on its own thread.
pub struct Worker {
    stop: Arc<AtomicBool>,
    pub events: Receiver<Event>,
}

impl Worker {
    /// Connect to the B-2 at `mac` and run a session; `wake` is called after each
    /// event so the window redraws.
    pub fn spawn(
        mac: String,
        properties: SessionProperties,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (send, events) = channel();
        let flag = Arc::clone(&stop);
        thread::spawn(move || {
            let send = |event: Event| {
                // The window closing drops the receiver; nothing is left to tell.
                let _ = send.send(event);
                wake();
            };
            if let Err(why) = session(&mac, &properties, &flag, &send) {
                send(Event::Failed(why));
            }
        });
        Self { stop, events }
    }

    /// Ask the session to end; it is stored like any other end.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn session(
    mac: &str,
    properties: &SessionProperties,
    stop: &AtomicBool,
    send: &dyn Fn(Event),
) -> Result<(), String> {
    let bank_dir = import::default_dir().ok_or("neither XDG_DATA_HOME nor HOME is set")?;
    let bank = audio::load_instrument(&bank_dir, INSTRUMENT_ID).map_err(|e| e.to_string())?;
    let store =
        Store::open(&storage::default_path().ok_or("neither XDG_DATA_HOME nor HOME is set")?)
            .map_err(|e| e.to_string())?;
    let mut sink = AlsaSink::open(OUTPUT_RATE_HZ).map_err(|e| format!("no audio output: {e}"))?;
    let stream = RfcommStream::connect(mac, DEFAULT_CHANNEL)
        .map_err(|e| format!("cannot connect to {mac}: {e}"))?;
    stream
        .set_read_timeout(READ_TIMEOUT)
        .map_err(|e| e.to_string())?;
    let session = Session::open(stream, live::stream_config()?).map_err(|e| e.to_string())?;
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
            report: &mut report,
        },
    );
    Ok(())
}
