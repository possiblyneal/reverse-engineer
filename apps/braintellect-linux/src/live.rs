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
//! Inputs the clone does not derive, and what a live run uses for each:
//! - The Evaluator's Coherence path is not implemented. The control code comes from its
//!   Balance path, which starts at the documented `defaultControlValue` (111) and is
//!   decided at each 60 s epoch end; the [`Snapshot`] reports the current code.
//! - The IPP scalar-head flags are all `true`: the original's order follows heap alignment
//!   per process, and all-scalar-head departs less from its logged order than all-SIMD
//!   (`audio-feedback.md`, "Live sum order").
//! - The counter timer starts one epoch ([`TRIGGER_EPOCH_MS`]) before the clock reading
//!   taken as the graph is built, so the first block is already eligible to fire, as in
//!   the original (`audio-feedback.md`, "Counter timer start"); both of its clock reads
//!   per block are the block's reading.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::acquisition::decode::{ChannelCalibration, DeviceFamily};
use crate::acquisition::session::{Error as SessionError, Session};
use crate::acquisition::stream::{
    SampleFrame, StreamConfig, StreamStats, CHANNELS as STREAM_CHANNELS,
};
use crate::audio::bank::Bank;
use crate::audio::player::Player;
use crate::audio::sink::{AlsaSink, SinkError};
use crate::feedback::{
    Interference1, Interference2, Mode, Smart2Design, Smart2Graph, SourceDesign, SourceFilters,
    BLOCK_LEN, CHANNELS, DEFAULT_CODE, RATE_HZ, TRIGGER_EPOCH_MS,
};
use crate::sensor::{SensorDesign, SensorMonitor, SensorStatus, BLOCK_SAMPLES};
use crate::storage::{Channel, Header, Point, Series, SessionRecord, Store};

/// Output rate of the sink: the sample rate of instrument bank 1002
/// (`audio-feedback.md`, "Wave device and instrument 1002").
pub const OUTPUT_RATE_HZ: u32 = 96_000;
/// The instrument the SMART-2 design plays.
pub const INSTRUMENT_ID: u32 = 1002;
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
/// Spacing of the stored series points (`tests/fixtures/record.tsv`).
const POINT_EVERY_MS: u64 = 5_000;
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
    fn source(&self) -> SourceDesign {
        SourceDesign {
            interference1: self.interference1,
            interference2: self.interference2,
            low_pass_hz: self.low_pass_hz,
            high_pass_hz: self.high_pass_hz,
        }
    }
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
    /// Milliseconds on a monotonic clock; read once per block and once per
    /// quiet read.
    pub clock: &'a mut dyn FnMut() -> i64,
    /// Called with the state on every change worth drawing.
    pub report: &'a mut dyn FnMut(&Snapshot),
}

/// Run one session to its end and return the final state, which has also been
/// reported. The session ends when `stop` is raised, the length is reached, or
/// the device closes the stream. It is stored if any frame arrived; an error
/// (the link, the audio device, the store) ends it without storing.
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
    let start_ms = (env.clock)();
    let built = Engine::new(properties, env.bank, start_ms);
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
        Ok(()) => match env.store.save(&engine.record(properties, family, model)) {
            Ok(id) => Phase::Saved { id },
            Err(e) => Phase::Failed(format!("the session could not be stored: {e}")),
        },
    };
    finish(&mut env, snapshot, phase)
}

/// The sensor status before a session (`sensor-status.md`): the Source output of an
/// opened B-2 runs through the BT-LOD design beside no SMART-2 graph, and `report`
/// gets the overlay state after each 512-sample block. Ends when `stop` is raised,
/// the link closes or errors; the stream is stopped on the way out. `clock` is read
/// once per block, for the `stateChangeDelay` debounce.
pub fn monitor<T: Read + Write>(
    mut session: Session<T>,
    properties: &SessionProperties,
    stop: &AtomicBool,
    clock: &mut dyn FnMut() -> i64,
    report: &mut dyn FnMut(SensorStatus),
) {
    let Ok(mut source) = (0..CHANNELS)
        .map(|_| SourceFilters::new(&properties.source(), RATE_HZ))
        .collect::<Result<Vec<_>, _>>()
    else {
        return;
    };
    if session.start(STREAM_CHANNELS as u8).is_err() {
        return;
    }
    let mut sensors = SensorMonitor::new(SensorDesign::bt_lod());
    sensors.set_engine_running(true);
    let mut samples = 0usize;
    while !stop.load(Ordering::Relaxed) {
        match session.next_frame() {
            Ok(Some(frame)) => {
                for n in 0..frame.channels[0].len() {
                    let volts: [f64; CHANNELS] =
                        std::array::from_fn(|c| source[c].push(frame.channels[c][n] * 1e-6));
                    samples += 1;
                    // The clock is read only for the sample that completes a block.
                    let now_ms = if samples.is_multiple_of(BLOCK_SAMPLES) {
                        clock()
                    } else {
                        0
                    };
                    if let Some(status) = sensors.push(now_ms, &volts) {
                        report(status);
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(SessionError::Io(e))
                if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {}
            Err(_) => break,
        }
    }
    // A link too broken to take Stop has nothing more to lose.
    let _ = session.disconnect();
}

fn finish(env: &mut Environment<'_>, mut snapshot: Snapshot, phase: Phase) -> Snapshot {
    snapshot.phase = phase;
    (env.report)(&snapshot);
    snapshot
}

/// The per-run processing state.
struct Engine<'a> {
    design: Smart2Design,
    source: Vec<SourceFilters>,
    graph: Smart2Graph,
    player: Player<'a>,
    /// Source output in volts, up to one block.
    block: Vec<[f64; CHANNELS]>,
    /// The last second of Source output per channel, microvolts.
    window: [VecDeque<f64>; CHANNELS],
    /// The latest amplitude of each band-pass, volts.
    latest_bands: Vec<f32>,
    /// Stored points per band-pass, in design order.
    points: Vec<Vec<Point>>,
    blocks: u64,
    start_ms: i64,
    now_ms: i64,
    last_frame_ms: i64,
}

impl<'a> Engine<'a> {
    fn new(properties: &SessionProperties, bank: &'a Bank, start_ms: i64) -> Result<Self, String> {
        let design = Smart2Design::smart2(properties.source());
        let source = (0..CHANNELS)
            .map(|_| SourceFilters::new(&design.source, RATE_HZ))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let graph = Smart2Graph::new(design.clone(), start_ms - TRIGGER_EPOCH_MS, Mode::Fixed)
            .map_err(|e| e.to_string())?;
        let bands = design.band_passes.len();
        Ok(Self {
            design,
            source,
            graph,
            player: Player::new(bank, OUTPUT_RATE_HZ, 0),
            block: Vec::with_capacity(BLOCK_LEN),
            window: std::array::from_fn(|_| VecDeque::with_capacity(RATE_HZ)),
            latest_bands: vec![0.0; bands],
            points: vec![Vec::new(); bands],
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
        let mut report = resumed;
        for n in 0..frame.channels[0].len() {
            let mut volts = [0.0; CHANNELS];
            for (c, chain) in self.source.iter_mut().enumerate() {
                volts[c] = chain.push(frame.channels[c][n] * 1e-6);
                let window = &mut self.window[c];
                if window.len() == RATE_HZ {
                    window.pop_front();
                }
                window.push_back(volts[c] * 1e6);
            }
            self.block.push(volts);
            if self.block.len() == BLOCK_LEN {
                self.process_block(env, snapshot)?;
                report |= self.blocks.is_multiple_of(REPORT_EVERY_BLOCKS);
            }
        }
        if report {
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
        self.last_frame_ms = self.now_ms;
        let decision = self.graph.process_evaluated_block(self.now_ms, &self.block);
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
        if let Some(last) = decision.samples.last() {
            self.latest_bands.clone_from(&last.bands);
        }
        self.blocks += 1;
        let at_ms = self.blocks * BLOCK_MS;
        if at_ms.is_multiple_of(POINT_EVERY_MS) {
            for (points, &volts) in self.points.iter_mut().zip(&self.latest_bands) {
                points.push(Point {
                    x: at_ms as f64,
                    y: f64::from(volts) * 1e6,
                });
            }
        }
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
        let channels = (0..CHANNELS)
            .map(|channel| Channel {
                name: format!("Channel{}", channel + 1),
                series: self
                    .design
                    .band_passes
                    .iter()
                    .zip(&self.points)
                    .filter(|(spec, _)| spec.channel == channel)
                    .map(|(spec, points)| Series {
                        name: spec.label.clone(),
                        points: points.clone(),
                    })
                    .collect(),
            })
            .collect();
        SessionRecord {
            header,
            duration_s: self.elapsed_s(),
            channels,
        }
    }
}

fn rms(window: &VecDeque<f64>) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    (window.iter().map(|v| v * v).sum::<f64>() / window.len() as f64).sqrt()
}
