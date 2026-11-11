//! The UI state model: no drawing, no threads, no devices.

use std::path::PathBuf;

use crate::acquisition::stream::CHANNELS;
use crate::audio::tone::{self, Availability, Tone, TONES};
use crate::fit::SensorFit;
use crate::live::{Health, Phase, SessionProperties, Snapshot};

use super::bluetooth::PairFailure;

/// The channel names of the montage `FP1FP2T3T4`, in the engine's channel order.
const CHANNEL_LABELS: [&str; 4] = ["FP1", "FP2", "T3", "T4"];

/// What the engine side tells the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The running session's state, including its final state.
    Update(Snapshot),
    /// The fit of FP1, FP2, T3 and T4 the engine reports while no session runs.
    Sensor([SensorFit; CHANNELS]),
    /// The sensor check ended: on a stop (`None`) or because it could not run.
    SensorCheckEnded(Option<String>),
    /// The session could not start, and why.
    Failed(String),
    /// A stop ended the session before it streamed.
    Cancelled,
    Imported,
    ImportFailed(String),
    /// The B-2 at this address is paired and trusted; the worker goes on to the
    /// sensor check, whose events end with [`Event::SensorCheckEnded`].
    Paired(String),
    /// Pairing failed; after [`PairFailure::BondLost`] the host has no bond, so the
    /// address is forgotten and Pair and connect is offered again.
    PairFailed(PairFailure),
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
    /// Find the B-2, pair and trust it, then show its sensor status.
    PairAndConnect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Setup,
    /// Looking for the B-2 and pairing with it.
    Pairing,
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
    /// No B-2 address is known and none is being paired.
    pub can_pair: bool,
    /// The B-2 address in use; empty until one is known.
    pub address: String,
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
    /// Each sensor's fit ("FP1 good, FP2 not touching, ..."), or "Not Available".
    pub sensor: String,
    pub channels: Vec<ChannelView>,
    pub frames: String,
    pub feedback: String,
    /// The Evaluator's current control code.
    pub control: String,
    pub elapsed: String,
}

pub struct UiModel {
    /// The B-2's address; `None` until it is passed in or paired.
    mac: Option<String>,
    /// Availability of each of [`TONES`], in order (`docs/adrs/0004-*.md`).
    tones: Vec<Availability>,
    /// Index into [`TONES`] of the tone a session plays.
    tone: usize,
    /// The session settings the clinician edits.
    pub properties: SessionProperties,
    stage: Stage,
    snapshot: Option<Snapshot>,
    sensor: Option<[SensorFit; CHANNELS]>,
    checking: bool,
    notice: String,
}

impl UiModel {
    /// A model for the B-2 at `mac`, with the default tone's availability `default`
    /// and every other tone not imported until [`Self::set_tones`].
    #[must_use]
    pub fn new(mac: String, default: Availability) -> Self {
        let mut model = Self::without_address(default);
        model.mac = Some(mac);
        model
    }

    /// A model that has no B-2 address yet: the window offers Pair and connect.
    #[must_use]
    pub fn without_address(default: Availability) -> Self {
        let notice = match &default {
            Availability::Ready => String::new(),
            Availability::Missing(why) => why.clone(),
        };
        let mut tones = vec![Availability::Missing("not imported".to_owned()); TONES.len()];
        let tone = tone::default_index();
        tones[tone] = default;
        Self {
            mac: None,
            tones,
            tone,
            properties: SessionProperties::default(),
            stage: Stage::Setup,
            snapshot: None,
            sensor: None,
            checking: false,
            notice,
        }
    }

    #[must_use]
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// The B-2's address, once known.
    #[must_use]
    pub fn address(&self) -> Option<&str> {
        self.mac.as_deref()
    }

    /// Start finding and pairing the B-2, then showing its sensor status. Only when
    /// no address is known and nothing else is under way.
    pub fn pair_and_connect(&mut self) -> Option<Command> {
        if !self.can_pair() {
            return None;
        }
        self.stage = Stage::Pairing;
        self.notice.clear();
        Some(Command::PairAndConnect)
    }

    /// The tone a session plays.
    #[must_use]
    pub fn tone(&self) -> &'static Tone {
        &TONES[self.tone]
    }

    /// Replace the availability of every tone, in [`TONES`] order.
    pub fn set_tones(&mut self, tones: Vec<Availability>) {
        assert_eq!(tones.len(), TONES.len(), "one availability per tone");
        self.tones = tones;
    }

    /// Choose the tone a session plays. Refused (`false`) when the tone is not
    /// imported or a session is connecting, running or checking sensors.
    pub fn select_tone(&mut self, tone: &Tone) -> bool {
        let Some(index) = TONES.iter().position(|t| t == tone) else {
            return false;
        };
        if self.tones[index] != Availability::Ready || !self.idle() {
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

    /// Start showing the sensor status from the B-2 stream. It plays no tone, so no
    /// tone needs to be imported.
    pub fn check_sensors(&mut self) -> Option<Command> {
        if !self.idle() || self.mac.is_none() {
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
            Event::Sensor(fit) => self.sensor = Some(fit),
            Event::SensorCheckEnded(why) => {
                self.checking = false;
                self.sensor = None;
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
                self.tones[tone::default_index()] = Availability::Ready;
                self.notice = "Instrument imported.".to_owned();
            }
            Event::ImportFailed(why) => self.notice = format!("Import failed: {why}"),
            Event::Paired(address) => {
                self.mac = Some(address);
                self.stage = Stage::Setup;
                self.checking = true;
            }
            Event::PairFailed(failure) => {
                if matches!(failure, PairFailure::BondLost(_)) {
                    self.mac = None;
                }
                self.stage = Stage::Setup;
                self.notice = failure.message();
            }
        }
    }

    #[must_use]
    pub fn view(&self) -> View {
        let snapshot = self.snapshot.as_ref();
        View {
            can_start: self.startable(),
            can_stop: matches!(self.stage, Stage::Connecting | Stage::Running),
            can_check: self.idle() && self.mac.is_some(),
            can_pair: self.can_pair(),
            address: self.mac.clone().unwrap_or_default(),
            checking: self.checking,
            needs_import: matches!(self.tones[tone::default_index()], Availability::Missing(_)),
            can_choose_tone: self.idle(),
            tones: TONES
                .iter()
                .zip(&self.tones)
                .enumerate()
                .map(|(i, (tone, state))| ToneView {
                    label: tone.label.to_owned(),
                    available: *state == Availability::Ready,
                    selected: i == self.tone,
                    status: match state {
                        Availability::Ready => String::new(),
                        Availability::Missing(why) => why.clone(),
                    },
                })
                .collect(),
            notice: self.notice.clone(),
            connection: self.connection(),
            sensor: self.sensor_label(),
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
    fn sensor_label(&self) -> String {
        match (&self.stage, self.sensor) {
            (Stage::Setup | Stage::Done, Some(fit)) => CHANNEL_LABELS
                .iter()
                .zip(fit)
                .map(|(label, fit)| format!("{label} {}", fit.label()))
                .collect::<Vec<_>>()
                .join(", "),
            _ => "Not Available".to_owned(),
        }
    }

    fn startable(&self) -> bool {
        self.tones[self.tone] == Availability::Ready && self.idle() && self.mac.is_some()
    }

    fn can_pair(&self) -> bool {
        self.mac.is_none() && self.idle()
    }

    /// No session, pairing or sensor check is under way.
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
            (Stage::Pairing, _) => "Looking for the B-2 and pairing".to_owned(),
            (Stage::Connecting, _) => {
                format!("Connecting to {}", self.mac.as_deref().unwrap_or_default())
            }
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
