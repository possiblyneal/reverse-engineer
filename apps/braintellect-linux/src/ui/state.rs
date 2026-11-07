//! The UI state model: no drawing, no threads, no devices.

use std::path::PathBuf;

use crate::audio::tone::{Availability, Tone, TONES};
use crate::live::{Health, Phase, SessionProperties, Snapshot};
use crate::sensor::SensorStatus;

/// The channel names of the montage `FP1FP2T3T4`, in the engine's channel order.
const CHANNEL_LABELS: [&str; 4] = ["FP1", "FP2", "T3", "T4"];

/// Whether a tone has been imported (`docs/adrs/0004-*.md`).
pub type Instrument = Availability;

/// What the engine side tells the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The running session's state, including its final state.
    Update(Snapshot),
    /// The sensor status the engine reports while no session runs.
    Sensor(SensorStatus),
    /// The sensor check ended: on a stop (`None`) or because it could not run.
    SensorCheckEnded(Option<String>),
    /// The session could not start, and why.
    Failed(String),
    /// A stop ended the session before it streamed.
    Cancelled,
    Imported,
    ImportFailed(String),
}

/// What the model asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Start(SessionProperties),
    Stop,
    /// Show the sensor status from the B-2 stream before a session.
    CheckSensors,
    StopCheck,
    /// Copy the instrument banks from the original's `lib` directory.
    Import(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Setup,
    Connecting,
    Running,
    /// The last session ended; its outcome is in the notice.
    Done,
}

/// One channel's live signal level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelView {
    pub label: String,
    pub level: String,
}

/// One tone in the set-up list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToneView {
    pub label: String,
    /// Imported, so it can be chosen.
    pub available: bool,
    pub selected: bool,
    /// Why the tone is unavailable; empty when it is not.
    pub status: String,
}

/// What the window shows, as text and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub can_start: bool,
    pub can_stop: bool,
    pub can_check: bool,
    /// The sensor check is streaming.
    pub checking: bool,
    /// The original's default bank is not imported yet.
    pub needs_import: bool,
    /// Every tone, in [`TONES`] order.
    pub tones: Vec<ToneView>,
    /// The tone can be changed: no session or sensor check is under way.
    pub can_choose_tone: bool,
    /// The latest outcome or problem; empty when there is none.
    pub notice: String,
    pub connection: String,
    /// "Ready", "Adjust Fit" or "Not Available".
    pub sensor: String,
    pub channels: Vec<ChannelView>,
    pub frames: String,
    pub feedback: String,
    /// The Evaluator's current control code.
    pub control: String,
    pub elapsed: String,
}

pub struct UiModel {
    mac: String,
    /// Availability of each of [`TONES`], in order.
    tones: Vec<Instrument>,
    /// Index into [`TONES`] of the tone a session plays.
    tone: usize,
    /// The session settings the clinician edits.
    pub properties: SessionProperties,
    stage: Stage,
    snapshot: Option<Snapshot>,
    sensor: SensorStatus,
    checking: bool,
    notice: String,
}

impl UiModel {
    #[must_use]
    pub fn new(mac: String, instrument: Instrument) -> Self {
        let notice = match &instrument {
            Instrument::Ready => String::new(),
            Instrument::Missing(why) => why.clone(),
        };
        let mut tones = vec![Instrument::Missing("not imported".to_owned()); TONES.len()];
        tones[0] = instrument;
        Self {
            mac,
            tones,
            tone: 0,
            properties: SessionProperties::default(),
            stage: Stage::Setup,
            snapshot: None,
            sensor: SensorStatus::NotAvailable,
            checking: false,
            notice,
        }
    }

    #[must_use]
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// The tone a session plays.
    #[must_use]
    pub fn tone(&self) -> &'static Tone {
        &TONES[self.tone]
    }

    /// Replace the availability of every tone, in [`TONES`] order.
    pub fn set_tones(&mut self, tones: Vec<Instrument>) {
        assert_eq!(tones.len(), TONES.len(), "one availability per tone");
        self.tones = tones;
    }

    /// Choose the tone a session plays. Refused (`false`) when the tone is not
    /// imported or a session is connecting, running or checking sensors.
    pub fn select_tone(&mut self, tone: &Tone) -> bool {
        let Some(index) = TONES.iter().position(|t| t == tone) else {
            return false;
        };
        if self.tones[index] != Instrument::Ready || !self.idle() {
            return false;
        }
        self.tone = index;
        true
    }

    /// Start a session with the current properties.
    pub fn start(&mut self) -> Option<Command> {
        if !self.startable() {
            return None;
        }
        if let Some(problem) = self.problem() {
            self.notice = problem.to_owned();
            return None;
        }
        self.stage = Stage::Connecting;
        self.snapshot = None;
        self.notice.clear();
        Some(Command::Start(self.properties.clone()))
    }

    /// Start showing the sensor status from the B-2 stream.
    pub fn check_sensors(&mut self) -> Option<Command> {
        if !self.startable() {
            return None;
        }
        self.checking = true;
        self.notice.clear();
        Some(Command::CheckSensors)
    }

    /// Stop the sensor check.
    #[must_use]
    pub fn stop_check(&self) -> Option<Command> {
        self.checking.then_some(Command::StopCheck)
    }

    /// Stop the session that is connecting or running.
    #[must_use]
    pub fn stop(&self) -> Option<Command> {
        matches!(self.stage, Stage::Connecting | Stage::Running).then_some(Command::Stop)
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Update(snapshot) => {
                match &snapshot.phase {
                    Phase::Running => self.stage = Stage::Running,
                    Phase::Saved { id } => {
                        self.stage = Stage::Done;
                        self.notice = format!(
                            "Session saved (record {id}, {}).",
                            clock(snapshot.elapsed_s)
                        );
                    }
                    Phase::Failed(why) => self.fail(why),
                }
                self.snapshot = Some(snapshot);
            }
            Event::Sensor(status) => self.sensor = status,
            Event::SensorCheckEnded(why) => {
                self.checking = false;
                self.sensor = SensorStatus::NotAvailable;
                if let Some(why) = why {
                    self.notice = format!("Sensor check stopped: {why}");
                }
            }
            Event::Failed(why) => self.fail(&why),
            Event::Cancelled => {
                self.stage = Stage::Setup;
                self.notice = "Cancelled.".to_owned();
            }
            Event::Imported => {
                self.tones[0] = Instrument::Ready;
                self.notice = "Instrument imported.".to_owned();
            }
            Event::ImportFailed(why) => self.notice = format!("Import failed: {why}"),
        }
    }

    #[must_use]
    pub fn view(&self) -> View {
        let snapshot = self.snapshot.as_ref();
        View {
            can_start: self.startable(),
            can_stop: matches!(self.stage, Stage::Connecting | Stage::Running),
            can_check: self.startable(),
            checking: self.checking,
            needs_import: matches!(self.tones[0], Instrument::Missing(_)),
            can_choose_tone: self.idle(),
            tones: TONES
                .iter()
                .zip(&self.tones)
                .enumerate()
                .map(|(i, (tone, state))| ToneView {
                    label: tone.label.to_owned(),
                    available: *state == Instrument::Ready,
                    selected: i == self.tone,
                    status: match state {
                        Instrument::Ready => String::new(),
                        Instrument::Missing(why) => why.clone(),
                    },
                })
                .collect(),
            notice: self.notice.clone(),
            connection: self.connection(),
            sensor: self.sensor_label().to_owned(),
            channels: CHANNEL_LABELS
                .iter()
                .enumerate()
                .map(|(i, label)| ChannelView {
                    label: (*label).to_owned(),
                    level: snapshot
                        .map_or_else(|| "-".to_owned(), |s| format!("{:.1} \u{b5}V", s.rms_uv[i])),
                })
                .collect(),
            frames: snapshot.map_or_else(String::new, |s| {
                format!(
                    "{} frames, {}, {}",
                    s.frames,
                    plural(s.stats.packet_counter_errors.into(), "counter error"),
                    plural(s.stats.sample_validation_errors.into(), "validation error"),
                )
            }),
            feedback: snapshot.map_or_else(String::new, |s| match s.last_note {
                Some(note) => format!("{}, last note {note}", plural(s.triggers.into(), "note")),
                None => "No notes yet".to_owned(),
            }),
            control: snapshot.map_or_else(String::new, |s| format!("Control code {}", s.control)),
            elapsed: snapshot.map_or_else(String::new, |s| {
                format!(
                    "{} of {}",
                    clock(s.elapsed_s),
                    clock(self.properties.duration_s)
                )
            }),
        }
    }

    /// The engine is off while a session connects or runs (`sensor-status.md`).
    fn sensor_label(&self) -> &'static str {
        match self.stage {
            Stage::Setup | Stage::Done => self.sensor.label(),
            Stage::Connecting | Stage::Running => SensorStatus::NotAvailable.label(),
        }
    }

    fn startable(&self) -> bool {
        self.tones[self.tone] == Instrument::Ready && self.idle()
    }

    /// No session or sensor check is under way.
    fn idle(&self) -> bool {
        !self.checking && matches!(self.stage, Stage::Setup | Stage::Done)
    }

    fn fail(&mut self, why: &str) {
        self.stage = Stage::Done;
        self.notice = format!("Session not saved: {why}");
    }

    fn problem(&self) -> Option<&'static str> {
        let p = &self.properties;
        if p.duration_s == 0 {
            Some("The session length must be at least 1 second.")
        } else if p.high_pass_hz >= p.low_pass_hz {
            Some("The high-pass cutoff must be below the low-pass cutoff.")
        } else {
            None
        }
    }

    fn connection(&self) -> String {
        match (&self.stage, self.snapshot.as_ref().map(|s| s.health)) {
            (Stage::Connecting, _) => format!("Connecting to {}", self.mac),
            (Stage::Running, Some(Health::Streaming)) => "Streaming".to_owned(),
            (Stage::Running, Some(Health::NoData)) => "No data from the B-2".to_owned(),
            (Stage::Running, _) => "Connected, waiting for data".to_owned(),
            (Stage::Setup | Stage::Done, _) => "Not connected".to_owned(),
        }
    }
}

fn plural(n: u64, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// `m:ss`.
fn clock(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
