//! The UI state model: no drawing, no threads, no devices.

use std::path::PathBuf;

use crate::live::{Health, Phase, SessionProperties, Snapshot};

/// The channel names of the montage `FP1FP2T3T4`, in the engine's channel order.
const CHANNEL_LABELS: [&str; 4] = ["FP1", "FP2", "T3", "T4"];

/// Whether instrument 1002 has been imported (`docs/adrs/0004-*.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instrument {
    Ready,
    /// Not imported yet; the message says where it should be.
    Missing(String),
}

/// What the engine side tells the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The running session's state, including its final state.
    Update(Snapshot),
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

/// What the window shows, as text and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub can_start: bool,
    pub can_stop: bool,
    pub needs_import: bool,
    /// The latest outcome or problem; empty when there is none.
    pub notice: String,
    pub connection: String,
    pub channels: Vec<ChannelView>,
    pub frames: String,
    pub feedback: String,
    pub elapsed: String,
}

pub struct UiModel {
    mac: String,
    instrument: Instrument,
    /// The session settings the clinician edits.
    pub properties: SessionProperties,
    stage: Stage,
    snapshot: Option<Snapshot>,
    notice: String,
}

impl UiModel {
    #[must_use]
    pub fn new(mac: String, instrument: Instrument) -> Self {
        let notice = match &instrument {
            Instrument::Ready => String::new(),
            Instrument::Missing(why) => why.clone(),
        };
        Self {
            mac,
            instrument,
            properties: SessionProperties::default(),
            stage: Stage::Setup,
            snapshot: None,
            notice,
        }
    }

    #[must_use]
    pub fn stage(&self) -> &Stage {
        &self.stage
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
            Event::Failed(why) => self.fail(&why),
            Event::Cancelled => {
                self.stage = Stage::Setup;
                self.notice = "Cancelled.".to_owned();
            }
            Event::Imported => {
                self.instrument = Instrument::Ready;
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
            needs_import: matches!(self.instrument, Instrument::Missing(_)),
            notice: self.notice.clone(),
            connection: self.connection(),
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
            elapsed: snapshot.map_or_else(String::new, |s| {
                format!(
                    "{} of {}",
                    clock(s.elapsed_s),
                    clock(self.properties.duration_s)
                )
            }),
        }
    }

    fn startable(&self) -> bool {
        self.instrument == Instrument::Ready && matches!(self.stage, Stage::Setup | Stage::Done)
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
