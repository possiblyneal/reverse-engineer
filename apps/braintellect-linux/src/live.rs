//! The live session engine: one session from an opened B-2 [`Session`] to a stored
//! record. Frames go through the Source filters and the SMART-2 graph, the
//! graph's triggers through the [`Player`] into an [`AudioSink`], and on stop the
//! session is saved to the [`Store`].
//!
//! [`run`] blocks until the session ends, so a caller runs it off the UI thread
//! and reads its progress from [`Environment::report`]. Everything outside the
//! process is injected: the transport (inside the `Session`), the sink, the
//! clock and the stop flag.
//!
//! The control code is the Evaluator's (Balance and Coherence paths): the documented
//! `defaultControlValue` (111) until the first 60 s epoch end decides one; the
//! [`Snapshot`] reports the current code.
//!
//! The Source filters run [`GraphStart::warmup_samples`] of the stream, with the interference
//! filter off, before the graph's first sample, as the original's run from connect to Play
//! (`audio-feedback.md`, "Live replay of the #33 capture").
//!
//! Inputs the clone does not derive, and what a live run ([`GraphStart::LIVE`]) uses for each:
//! - The IPP scalar-head flags are all `true`: the original's order follows heap alignment
//!   per process, and all-scalar-head departs less from its logged order than all-SIMD
//!   (`audio-feedback.md`, "Live sum order").
//! - The counter timer starts one epoch ([`TRIGGER_EPOCH_MS`]) before the clock reading
//!   taken as the graph is built, so the first block is already eligible to fire, as in
//!   the original (`audio-feedback.md`, "Counter timer start"). It reads the clock once per
//!   block and again when it fires, to restart.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::acquisition::decode::{ChannelCalibration, DeviceFamily};
use crate::acquisition::session::{Error as SessionError, Session};
use crate::acquisition::stream::{
    SampleFrame, StreamConfig, StreamStats, CHANNELS as STREAM_CHANNELS,
};
use crate::audio::bank::Bank;
use crate::audio::player::Player;
use crate::audio::sink::{AlsaSink, SinkError};
use crate::feedback::{
    CounterClock, Interference1, Interference2, Mode, Smart2Design, Smart2Graph, SourceDesign,
    SourceFilters, StoredSeries, BLOCK_LEN, CHANNELS, DEFAULT_CODE, RATE_HZ, TRIGGER_EPOCH_MS,
};
use crate::fit::{FitMonitor, SensorFit};
use crate::sensor::{SensorDesign, SensorMonitor, SensorStatus, TICK_MS};
use crate::storage::{Channel, Header, SessionRecord, Store};

/// Output rate of the sink: the sample rate of instrument bank 1002
/// (`audio-feedback.md`, "Wave device and instrument 1002").
pub const OUTPUT_RATE_HZ: u32 = 96_000;
/// Baseline calibration (`b2-protocol.md`, "Calibration confirmed"):
/// channelGain 12, sensorGain 10, as the caller supplies them (not read from the device).
pub const CHANNEL_GAIN: f64 = 12.0;
pub const SENSOR_GAIN: f64 = 10.0;

/// The stream configuration of every run.
///
/// # Errors
/// Never, for the constants above; the calibration check is the decoder's.
pub fn stream_config() -> Result<StreamConfig, String> {
    Ok(StreamConfig {
        calibration: ChannelCalibration::new(CHANNEL_GAIN, SENSOR_GAIN)
            .map_err(|e| format!("{e:?}"))?,
        family: None,
    })
}

/// A link with no frame for this long reads as [`Health::NoData`].
const NO_DATA_MS: i64 = 2_000;
/// Blocks between status reports while frames arrive (200 ms).
const REPORT_EVERY_BLOCKS: u64 = 25;
/// Stream time of one block.
const BLOCK_MS: u64 = (BLOCK_LEN * 1000 / RATE_HZ) as u64;
/// Output frames of one block.
const BLOCK_OUTPUT_FRAMES: usize = (OUTPUT_RATE_HZ as usize * BLOCK_MS as usize) / 1000;

/// The device that plays audio.
pub trait AudioSink {
    /// Play `frames` (mono, [`OUTPUT_RATE_HZ`]), blocking until they are taken.
    ///
    /// # Errors
    /// Fails when the device refuses the write.
    fn write(&mut self, frames: &[i16]) -> Result<(), SinkError>;
}

impl AudioSink for AlsaSink {
    fn write(&mut self, frames: &[i16]) -> Result<(), SinkError> {
        AlsaSink::write(self, frames)
    }
}

/// The session lengths the original offers, in minutes (its "N minutes" strings,
/// file offset 17770132 of `braintellect.exe`). The default 240 s is the first.
pub const SESSION_MINUTES: [u32; 7] = [4, 6, 8, 10, 20, 40, 60];

/// What the clinician sets for a session: the Source device settings the SMART-2
/// run uses and the session length. The defaults are the run-8 baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionProperties {
    pub interference1: Interference1,
    pub interference2: Interference2,
    pub low_pass_hz: f64,
    pub high_pass_hz: f64,
    /// Session length in seconds, on the clock.
    pub duration_s: u32,
}

impl Default for SessionProperties {
    fn default() -> Self {
        Self {
            interference1: Interference1::LowPass45,
            interference2: Interference2::Hz25,
            low_pass_hz: 150.0,
            high_pass_hz: 1.5,
            duration_s: 240,
        }
    }
}

impl SessionProperties {
    /// The Source settings of the BT-LOD sensor check: the original's LOD Source runs with
    /// the 45 Hz low-pass off and the 25 Hz band-stop on, whatever the session's
    /// interference setting (2026-11-08 worn replays; `sensor-status.md`). The low-pass and
    /// high-pass cutoffs follow the session's; the replays covered only the defaults, so
    /// whether the original's do is unresolved (`sensor-status.md`, Unresolved).
    #[must_use]
    pub fn lod_source(&self) -> SourceDesign {
        SourceDesign {
            interference1: Interference1::Off,
            interference2: Interference2::Hz25,
            ..self.source()
        }
    }

    fn source(&self) -> SourceDesign {
        SourceDesign {
            interference1: self.interference1,
            interference2: self.interference2,
            low_pass_hz: self.low_pass_hz,
            high_pass_hz: self.high_pass_hz,
        }
    }
}

/// How the feedback graph starts on the stream. A live session runs [`GraphStart::LIVE`]; a
/// replay of the original's run supplies the original's behaviour and its thread-timed
/// inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphStart {
    pub mode: Mode,
    /// Samples the Source filters run, with the interference filter off, before the
    /// graph's first sample. The original streams from connect and starts the graph at
    /// Play, so its Source filters are warm there.
    pub warmup_samples: usize,
    /// Graph samples before the session's interference filter switches in, in
    /// [`Mode::Original`] (the original's is thread-timed: 1 in run 8, 4 in the #48 and #33
    /// replays); [`Mode::Fixed`] switches it in at the first graph sample.
    pub interference_switch: usize,
}

impl GraphStart {
    /// Fixed mode after one second of warm-up, twice the 256 samples that gave the #33
    /// capture the decisions of a 31 s warm-up (`audio-feedback.md`, "Live replay of the
    /// #33 capture").
    pub const LIVE: Self = Self {
        mode: Mode::Fixed,
        warmup_samples: RATE_HZ,
        interference_switch: 0,
    };
}

/// Where a session is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Running,
    /// The session ended and was stored as `id`.
    Saved {
        id: i64,
    },
    /// The session ended without being stored, and why.
    Failed(String),
}

/// What the link is delivering while [`Phase::Running`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Streaming was started and no frame has arrived yet.
    WaitingForData,
    Streaming,
    /// Frames stopped arriving.
    NoData,
}

/// Everything the UI shows about a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    pub health: Health,
    /// Valid frames received.
    pub frames: u64,
    pub stats: StreamStats,
    /// RMS of each channel's Source output over the last second, in microvolts.
    pub rms_uv: [f64; STREAM_CHANNELS],
    /// Notes triggered so far.
    pub triggers: u32,
    pub last_note: Option<i32>,
    /// The Evaluator's current control code.
    pub control: u32,
    /// Seconds on the clock since the session started.
    pub elapsed_s: u32,
}

/// What a run is wired to.
pub struct Environment<'a> {
    pub bank: &'a Bank,
    pub sink: &'a mut dyn AudioSink,
    pub store: &'a Store,
    /// Raised to end the session; it is stored like any other end.
    pub stop: &'a AtomicBool,
    /// Milliseconds on a monotonic clock; read as the graph is built, once per block,
    /// again in a block where the counter fires, and once per quiet read.
    pub clock: &'a mut dyn FnMut() -> i64,
    /// How the graph starts: [`GraphStart::LIVE`] for a live session.
    pub graph: GraphStart,
    /// Called with the state on every change worth drawing.
    pub report: &'a mut dyn FnMut(&Snapshot),
    /// The raw capture file, relative to the data directory, stored with the session.
    pub capture: Option<String>,
}

/// Run one session to its end and return the final state, which has also been
/// reported. The session ends when `stop` is raised, the length (counted from the graph's
/// first block, after the warm-up) is reached, or the device closes the stream. It is
/// stored if any frame arrived; an error (the link, the audio device, the store) ends it
/// without storing.
pub fn run<T: Read + Write>(
    mut session: Session<T>,
    properties: &SessionProperties,
    mut env: Environment<'_>,
) -> Snapshot {
    let mut snapshot = Snapshot {
        phase: Phase::Running,
        health: Health::WaitingForData,
        frames: 0,
        stats: StreamStats::default(),
        rms_uv: [0.0; STREAM_CHANNELS],
        triggers: 0,
        last_note: None,
        control: DEFAULT_CODE,
        elapsed_s: 0,
    };
    let (family, model) = (session.family(), session.info().system1.model);
    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let start_ms = (env.clock)();
    let built = ChannelCalibration::new(CHANNEL_GAIN, SENSOR_GAIN)
        .map_err(|e| format!("{e:?}"))
        .and_then(|calibration| {
            Engine::new(
                properties,
                env.graph,
                env.bank,
                start_ms,
                calibration.divisor(family),
            )
        });
    let mut engine = match built {
        Ok(engine) => engine,
        Err(why) => return finish(&mut env, snapshot, Phase::Failed(why)),
    };
    if let Err(e) = session.start(STREAM_CHANNELS as u8) {
        return finish(&mut env, snapshot, Phase::Failed(e.to_string()));
    }
    (env.report)(&snapshot);

    let length_ms = i64::from(properties.duration_s) * 1000;
    let mut stopping = false;
    let ended = loop {
        if env.stop.load(Ordering::Relaxed) {
            stopping = true;
            break Ok(());
        }
        match session.next_frame() {
            Ok(Some(frame)) => {
                if let Err(why) = engine.on_frame(&frame, &mut env, &mut snapshot) {
                    break Err(why);
                }
                snapshot.stats = session.stats();
            }
            Ok(None) => break Ok(()),
            Err(SessionError::Io(e))
                if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                engine.on_quiet((env.clock)(), &mut env, &mut snapshot);
            }
            Err(e) => break Err(e.to_string()),
        }
        if engine.elapsed_ms() >= length_ms {
            stopping = true;
            break Ok(());
        }
    };
    snapshot.stats = session.stats();
    snapshot.elapsed_s = engine.elapsed_s();
    if stopping {
        // The data is already in hand; a link too broken to take Stop does not
        // lose the session.
        let _ = session.disconnect();
    }
    let phase = match ended {
        Err(why) => Phase::Failed(why),
        Ok(()) if snapshot.frames == 0 => {
            Phase::Failed("no data was received from the B-2".to_owned())
        }
        Ok(()) => match env.store.save(&engine.record(
            properties,
            family,
            model,
            started_at,
            env.capture.clone(),
        )) {
            Ok(id) => Phase::Saved { id },
            Err(e) => Phase::Failed(format!("the session could not be stored: {e}")),
        },
    };
    finish(&mut env, snapshot, phase)
}

/// What [`monitor`] reports at each display tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorReport {
    /// The original's BT-LOD overlay state.
    pub overlay: SensorStatus,
    /// FP1, FP2, T3 and T4 after the last completed block; `None` before the first.
    pub fit: Option<[SensorFit; CHANNELS]>,
    /// Percent of frames the link lost over the last [`LOSS_WINDOW_MS`], rounded down; 100
    /// when nothing arrived in it. `None` before the first window ends.
    pub lost_percent: Option<u8>,
}

/// How long the sensor check measures frame loss over before it reports a figure.
pub const LOSS_WINDOW_MS: i64 = 3_000;

/// Percent of the frames due that were lost, rounded down; a window with nothing
/// received is a stalled link, all lost.
fn lost_percent(received: u32, lost: u32) -> u8 {
    if received == 0 {
        return 100;
    }
    let due = u64::from(received) + u64::from(lost);
    u8::try_from(u64::from(lost) * 100 / due).unwrap_or(100)
}

/// The sensor status before a session (`sensor-status.md`): the Source output of an
/// opened B-2 runs through the BT-LOD design and the fixed-mode fit ([`FitMonitor`]), with no
/// SMART-2 graph and no audio. The display timer ticks every [`TICK_MS`] (from `clock`, in
/// milliseconds) and `report` gets a [`SensorReport`] at each tick: the overlay state, as the
/// original's display re-evaluates it between blocks, the fit after the last completed block,
/// and the frame loss over the last [`LOSS_WINDOW_MS`]. Ends when `stop` is raised or the link
/// closes; the stream is stopped on the way out.
/// `clock` is read once at the start, then after every frame and quiet read.
///
/// # Errors
/// Why the check could not run or broke off: the Source filters cannot be built, the
/// stream does not start, or the link fails.
pub fn monitor<T: Read + Write>(
    mut session: Session<T>,
    properties: &SessionProperties,
    stop: &AtomicBool,
    clock: &mut dyn FnMut() -> i64,
    report: &mut dyn FnMut(SensorReport),
) -> Result<(), String> {
    let mut source = (0..CHANNELS)
        .map(|_| SourceFilters::new(&properties.lod_source(), RATE_HZ))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    session
        .start(STREAM_CHANNELS as u8)
        .map_err(|e| e.to_string())?;
    let ended = sensor_loop(&mut session, &mut source, stop, clock, report);
    // A link too broken to take Stop has nothing more to lose.
    let _ = session.disconnect();
    ended
}

/// The monitor's frame loop, until a stop, the end of the stream or a link failure.
fn sensor_loop<T: Read + Write>(
    session: &mut Session<T>,
    source: &mut [SourceFilters],
    stop: &AtomicBool,
    clock: &mut dyn FnMut() -> i64,
    report: &mut dyn FnMut(SensorReport),
) -> Result<(), String> {
    let mut sensors = SensorMonitor::new(SensorDesign::bt_lod());
    sensors.set_engine_running(true);
    let mut fits = FitMonitor::new();
    let mut fit = None;
    let start_ms = clock();
    let mut next_tick = start_ms + TICK_MS;
    let mut window_end = start_ms + LOSS_WINDOW_MS;
    let (mut received, mut lost_before) = (0, session.stats().lost_frames);
    let mut lost = None;
    while !stop.load(Ordering::Relaxed) {
        match session.next_frame() {
            Ok(Some(frame)) => {
                received += 1;
                for n in 0..frame.channels[0].len() {
                    let volts: [f64; CHANNELS] =
                        std::array::from_fn(|c| source[c].push(frame.channels[c][n] * 1e-6));
                    sensors.push(&volts);
                    fit = fits.push(&volts).or(fit);
                }
            }
            Ok(None) => return Ok(()),
            Err(SessionError::Io(e))
                if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {}
            Err(e) => return Err(e.to_string()),
        }
        let now_ms = clock();
        if now_ms >= window_end {
            let lost_total = session.stats().lost_frames;
            lost = Some(lost_percent(received, lost_total - lost_before));
            (window_end, received, lost_before) = (now_ms + LOSS_WINDOW_MS, 0, lost_total);
        }
        if now_ms >= next_tick {
            next_tick = now_ms + TICK_MS;
            report(SensorReport {
                overlay: sensors.tick(now_ms),
                fit,
                lost_percent: lost,
            });
        }
    }
    Ok(())
}

fn finish(env: &mut Environment<'_>, mut snapshot: Snapshot, phase: Phase) -> Snapshot {
    snapshot.phase = phase;
    (env.report)(&snapshot);
    snapshot
}

/// The per-run processing state.
struct Engine<'a> {
    source: Vec<SourceFilters>,
    graph: Smart2Graph,
    player: Player<'a>,
    /// Source output in volts, up to one block.
    block: Vec<[f64; CHANNELS]>,
    /// The last second of Source output per channel, microvolts.
    window: [VecDeque<f64>; CHANNELS],
    /// The stored series, fed the Source output from the graph's first sample.
    stored: StoredSeries,
    /// Stream samples still to warm the Source filters before the graph starts.
    warmup_left: usize,
    /// Graph samples before the session's interference filter switches in.
    switch_left: Option<usize>,
    /// The session's interference filter, switched in after `switch_left`.
    interference1: Interference1,
    blocks: u64,
    /// The clock at the graph's construction, then at its first block.
    start_ms: i64,
    now_ms: i64,
    last_frame_ms: i64,
}

impl<'a> Engine<'a> {
    fn new(
        properties: &SessionProperties,
        start: GraphStart,
        bank: &'a Bank,
        start_ms: i64,
        counts_per_volt: f64,
    ) -> Result<Self, String> {
        let design = Smart2Design::smart2(properties.source());
        // The interference filter is off until the graph starts, as in the original before Play.
        let warm = SourceDesign {
            interference1: Interference1::Off,
            ..design.source.clone()
        };
        let source = (0..CHANNELS)
            .map(|_| SourceFilters::new(&warm, RATE_HZ))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut graph = Smart2Graph::new(design.clone(), start_ms - TRIGGER_EPOCH_MS, start.mode)
            .map_err(|e| e.to_string())?;
        let bands = design.band_passes.len();
        // The live sum order in either mode: original mode defaults to all-SIMD.
        graph.set_scalar_heads(&vec![true; bands]);
        Ok(Self {
            source,
            graph,
            player: Player::new(bank, OUTPUT_RATE_HZ, 0),
            block: Vec::with_capacity(BLOCK_LEN),
            window: std::array::from_fn(|_| VecDeque::with_capacity(RATE_HZ)),
            stored: StoredSeries::new(counts_per_volt, RATE_HZ),
            warmup_left: start.warmup_samples,
            switch_left: Some(start.mode.interference_switch(0, start.interference_switch)),
            interference1: design.source.interference1,
            blocks: 0,
            start_ms,
            now_ms: start_ms,
            last_frame_ms: start_ms,
        })
    }

    fn elapsed_ms(&self) -> i64 {
        self.now_ms - self.start_ms
    }

    fn elapsed_s(&self) -> u32 {
        u32::try_from(self.elapsed_ms().max(0) / 1000).unwrap_or(u32::MAX)
    }

    fn on_quiet(&mut self, now_ms: i64, env: &mut Environment<'_>, snapshot: &mut Snapshot) {
        self.now_ms = now_ms;
        if snapshot.health != Health::NoData && now_ms - self.last_frame_ms >= NO_DATA_MS {
            snapshot.health = Health::NoData;
            snapshot.elapsed_s = self.elapsed_s();
            (env.report)(snapshot);
        }
    }

    fn on_frame(
        &mut self,
        frame: &SampleFrame,
        env: &mut Environment<'_>,
        snapshot: &mut Snapshot,
    ) -> Result<(), String> {
        snapshot.frames += 1;
        let resumed = snapshot.health != Health::Streaming;
        snapshot.health = Health::Streaming;
        let (blocks, control, triggers) = (self.blocks, snapshot.control, snapshot.triggers);
        for n in 0..frame.channels[0].len() {
            let mut volts = [0.0; CHANNELS];
            if self.warmup_left == 0 && self.switch_left == Some(0) {
                for chain in &mut self.source {
                    chain.set_interference1(self.interference1, RATE_HZ);
                }
            }
            for (c, chain) in self.source.iter_mut().enumerate() {
                volts[c] = chain.push(frame.channels[c][n] * 1e-6);
                let window = &mut self.window[c];
                if window.len() == RATE_HZ {
                    window.pop_front();
                }
                window.push_back(volts[c] * 1e6);
            }
            if self.warmup_left > 0 {
                self.warmup_left -= 1;
                continue;
            }
            self.stored.push(&volts);
            self.switch_left = self.switch_left.and_then(|left| left.checked_sub(1));
            self.block.push(volts);
            if self.block.len() == BLOCK_LEN {
                self.process_block(env, snapshot)?;
            }
        }
        // The latest clock reading, so a frame in the warm-up, which reads no clock, also counts.
        self.last_frame_ms = self.now_ms;
        // A new code or a trigger is drawn in the frame it happens; the rest every few blocks.
        if resumed
            || snapshot.control != control
            || snapshot.triggers != triggers
            || self.blocks != blocks && self.blocks.is_multiple_of(REPORT_EVERY_BLOCKS)
        {
            snapshot.rms_uv = self.window.each_ref().map(rms);
            snapshot.elapsed_s = self.elapsed_s();
            (env.report)(snapshot);
        }
        Ok(())
    }

    fn process_block(
        &mut self,
        env: &mut Environment<'_>,
        snapshot: &mut Snapshot,
    ) -> Result<(), String> {
        self.now_ms = (env.clock)();
        if self.blocks == 0 {
            self.start_ms = self.now_ms;
        }
        let clock = LiveClock {
            read_ms: self.now_ms,
            clock: &mut *env.clock,
        };
        let decision = self.graph.process_evaluated_block(clock, &self.block);
        snapshot.control = decision.control;
        self.block.clear();
        let stream_ms = self.blocks * BLOCK_MS;
        let block = usize::try_from(self.blocks).map_err(|e| e.to_string())?;
        let time_ms = i64::try_from(stream_ms).map_err(|e| e.to_string())?;
        self.player
            .feed(block, time_ms, decision.note_at_trigger)
            .map_err(|e| e.to_string())?;
        if let Some(note) = decision.note_at_trigger {
            snapshot.triggers += 1;
            snapshot.last_note = Some(note);
        }
        let first_frame = self.blocks * BLOCK_OUTPUT_FRAMES as u64;
        env.sink
            .write(&self.player.mix(first_frame, BLOCK_OUTPUT_FRAMES))
            .map_err(|e| e.to_string())?;
        self.blocks += 1;
        Ok(())
    }

    /// The session as stored. The header fields are the baseline record's
    /// (`tests/fixtures/record.tsv`): the app version and design, the montage,
    /// the calibration constants above and the device settings in use.
    fn record(
        &self,
        properties: &SessionProperties,
        family: DeviceFamily,
        model: [u16; 2],
        started_at: i64,
        capture: Option<String>,
    ) -> SessionRecord {
        let header = Header {
            version: "5.1.57".to_owned(),
            design: "BT-SMART-2-v5.1.2".to_owned(),
            montage: "FP1FP2T3T4".to_owned(),
            device: "B-2".to_owned(),
            samples_per_second: RATE_HZ as u32,
            samples_per_packet: BLOCK_LEN as u32,
            number_channels: CHANNELS as u32,
            device_gain: CHANNEL_GAIN,
            sensor_gain: SENSOR_GAIN,
            device_family: family.code(),
            device_model: format!("{:04}.{:04}", model[0], model[1]),
            interference_filter: match properties.interference1 {
                Interference1::Off => "Off",
                Interference1::LowPass45 => "45Hz LP",
            }
            .to_owned(),
            device_high_pass_cutoff: properties.high_pass_hz,
            device_low_pass_cutoff: properties.low_pass_hz,
            data_storage_style: "filtered".to_owned(),
        };
        let channels = self
            .stored
            .series()
            .into_iter()
            .enumerate()
            .map(|(channel, series)| Channel {
                name: format!("Channel{}", channel + 1),
                series,
            })
            .collect();
        SessionRecord {
            header,
            duration_s: self.elapsed_s(),
            started_at,
            capture,
            channels,
        }
    }
}

/// The counter's clock in a live block: the block's read, and a fresh read on a fire.
struct LiveClock<'c> {
    read_ms: i64,
    clock: &'c mut dyn FnMut() -> i64,
}

impl CounterClock for LiveClock<'_> {
    fn read_ms(&self) -> i64 {
        self.read_ms
    }

    fn lap_ms(&mut self) -> i64 {
        (self.clock)()
    }
}

fn rms(window: &VecDeque<f64>) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    (window.iter().map(|v| v * v).sum::<f64>() / window.len() as f64).sqrt()
}
