//! The egui window. It draws [`UiModel`], turns widget input into model calls and
//! [`Command`]s, and carries out the import and the tone check by calling
//! `import::import_banks` and `Tone::availability` (tested in their own modules).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::audio::import;
use crate::audio::tone::{self, Availability, TONES};
use crate::feedback::{Interference1, Interference2};
use crate::live::{Phase, SESSION_MINUTES};
use crate::storage::{self, Store, Summary};

use super::bluetooth::BluetoothHost;
use super::bluez::Bluez;
use super::history;
use super::state::{Command, Event, UiModel};
use super::worker::Worker;

/// The wait between presence probes while the B-2 is off. Each probe pages for about
/// 5 s, so the radio pages about half the time while it stays off; a B-2 that is on
/// keeps the probe's link, so nothing pages then.
const PROBE_EVERY: Duration = Duration::from_secs(5);

/// Open the window and run until it is closed. The B-2 is at `mac`, or else the
/// one BlueZ has paired, or else the window offers Pair and connect.
///
/// # Errors
/// Fails if the window cannot be created.
pub fn run(mac: Option<String>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BRAINtellect2")
            .with_inner_size([520.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "braintellect-linux",
        options,
        Box::new(|_| Ok(Box::new(App::new(mac)))),
    )
}

struct App {
    model: UiModel,
    worker: Option<Worker>,
    /// The tone preview playing or last played.
    preview: Option<Worker>,
    /// The presence probe running or last run, and when the next may start.
    probe: Option<Worker>,
    next_probe: Instant,
    /// Commands held until the probe lets go of the link, run in order once it does.
    pending: Vec<Command>,
    lib_dir: String,
    /// The stored sessions, or why they could not be read.
    history: Result<Vec<Summary>, String>,
    /// Why the last Mark as test / Not a test failed, until the next one.
    mark_failed: Option<String>,
    show_tests: bool,
    /// The session the last run stored, until the next run starts.
    saved: Option<i64>,
}

impl App {
    fn new(mac: Option<String>) -> Self {
        let tones = tone_availability();
        let default = tones[tone::default_index()].clone();
        let mut model = match mac.or_else(paired_b2) {
            Some(mac) => UiModel::new(mac, default),
            None => UiModel::without_address(default),
        };
        model.set_tones(tones);
        Self {
            model,
            worker: None,
            preview: None,
            probe: None,
            next_probe: Instant::now(),
            pending: Vec::new(),
            lib_dir: String::new(),
            history: read_history(),
            mark_failed: None,
            show_tests: false,
            saved: None,
        }
    }

    /// Mark a stored session as a test run or clear the mark, then list the sessions again.
    /// A failure keeps the list shown, so the mark can be tried again, and says why.
    fn mark_test(&mut self, id: i64, test: bool) {
        self.mark_failed = None;
        match open_store().and_then(|store| store.set_test(id, test).map_err(|e| e.to_string())) {
            Ok(()) => self.history = read_history(),
            Err(why) => self.mark_failed = Some(why),
        }
    }

    /// Run `command` once the probe has let go of the link, so the two links never meet.
    fn submit(&mut self, command: Command, ctx: &egui::Context) {
        if let Some(probe) = &self.probe {
            probe.stop();
        }
        self.pending.push(command);
        self.run_held(ctx);
    }

    /// Run the held commands once the probe has let go of the link.
    fn run_held(&mut self, ctx: &egui::Context) {
        if running(self.probe.as_ref()) {
            if !self.pending.is_empty() {
                // A stopped probe ends without an event, so look again shortly.
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return;
        }
        for command in std::mem::take(&mut self.pending) {
            self.run(command, ctx);
        }
    }

    /// Start a presence probe when the window is idle with a known B-2 and the wait is over.
    fn probe_when_idle(&mut self, ctx: &egui::Context) {
        let idle = self.model.view().can_check
            && self.pending.is_empty()
            && !running(self.worker.as_ref())
            && !running(self.probe.as_ref());
        if !idle {
            return;
        }
        let wait = self.next_probe.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        let mac = self.model.address().unwrap_or_default().to_owned();
        let ctx = ctx.clone();
        self.probe = Some(Worker::spawn_probe(mac, move || ctx.request_repaint()));
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        let mac = self.model.address().unwrap_or_default().to_owned();
        match command {
            Command::Start(properties) => {
                let ctx = ctx.clone();
                self.worker = Some(Worker::spawn(
                    mac,
                    properties,
                    self.model.tone(),
                    move || ctx.request_repaint(),
                ));
            }
            Command::CheckSensors => {
                let ctx = ctx.clone();
                self.worker = Some(Worker::spawn_check(
                    mac,
                    self.model.properties.clone(),
                    move || ctx.request_repaint(),
                ));
            }
            Command::PairAndConnect => {
                let ctx = ctx.clone();
                self.worker = Some(Worker::spawn_pair_and_connect(
                    self.model.properties.clone(),
                    move || ctx.request_repaint(),
                ));
            }
            Command::StopCheck | Command::Stop => {
                if let Some(worker) = &self.worker {
                    worker.stop();
                }
            }
            Command::Import(lib_dir) => {
                let event = match import::default_dir()
                    .ok_or_else(|| storage::NO_DATA_DIR.to_owned())
                    .and_then(|dest| {
                        import::import_banks(&lib_dir, &dest).map_err(|e| e.to_string())
                    }) {
                    Ok(_) => Event::Imported,
                    Err(why) => Event::ImportFailed(why),
                };
                self.model.apply(event);
                self.model.set_tones(tone_availability());
            }
        }
    }

    fn drain_events(&mut self) {
        let mut saved = false;
        for worker in [&self.worker, &self.preview].into_iter().flatten() {
            while let Ok(event) = worker.events.try_recv() {
                if let Event::Update(s) = &event {
                    self.saved = match s.phase {
                        Phase::Saved { id } => Some(id),
                        _ => None,
                    };
                    saved |= self.saved.is_some();
                }
                self.model.apply(event);
            }
        }
        if saved {
            self.history = read_history();
        }
        if let Some(probe) = &self.probe {
            while let Ok(event) = probe.events.try_recv() {
                self.model.apply(event);
                self.next_probe = Instant::now() + PROBE_EVERY;
            }
        }
    }

    /// The Past sessions section; the session the clinician asked to mark or unmark, if any.
    fn past_sessions(&mut self, ui: &mut egui::Ui) -> Option<(i64, bool)> {
        let mut mark = None;
        egui::CollapsingHeader::new("Past sessions").show(ui, |ui| {
            ui.checkbox(&mut self.show_tests, "Show test sessions");
            if let Some(why) = &self.mark_failed {
                ui.label(format!("The mark could not be saved: {why}"));
            }
            match &self.history {
                Err(why) => {
                    ui.label(format!("Past sessions could not be read: {why}"));
                }
                Ok(sessions) => {
                    let rows = history::rows(sessions, self.show_tests, history::local_time);
                    if rows.is_empty() {
                        ui.label("No sessions stored yet.");
                    }
                    for row in rows {
                        egui::CollapsingHeader::new(&row.title)
                            .id_salt(("session", row.id))
                            .show(ui, |ui| {
                                egui::Grid::new(("results", row.id)).show(ui, |ui| {
                                    ui.label("Band (µV)");
                                    for sensor in &row.sensors {
                                        ui.label(sensor);
                                    }
                                    ui.end_row();
                                    for (band, means) in &row.bands {
                                        ui.label(band);
                                        for mean in means {
                                            ui.label(mean);
                                        }
                                        ui.end_row();
                                    }
                                });
                                balance_grid(ui, ("balance", row.id), &row.balance);
                                let label = if row.test {
                                    "Not a test"
                                } else {
                                    "Mark as test"
                                };
                                if ui.button(label).clicked() {
                                    mark = Some((row.id, !row.test));
                                }
                            });
                    }
                }
            }
        });
        mark
    }

    fn previewing(&self) -> bool {
        running(self.preview.as_ref())
    }

    fn play_preview(&mut self, tone: &'static tone::Tone, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.preview = Some(Worker::spawn_preview(tone, move || ctx.request_repaint()));
    }
}

fn open_store() -> Result<Store, String> {
    let path = storage::default_path().ok_or(storage::NO_DATA_DIR)?;
    Store::open(&path).map_err(|e| e.to_string())
}

fn read_history() -> Result<Vec<Summary>, String> {
    open_store()?.summaries().map_err(|e| e.to_string())
}

fn running(worker: Option<&Worker>) -> bool {
    worker.is_some_and(|w| !w.is_finished())
}

/// The B-2 BlueZ has already paired, if BlueZ can be asked.
fn paired_b2() -> Option<String> {
    Bluez::connect()
        .and_then(|mut host| host.paired())
        .ok()
        .flatten()
}

/// Which of the tones are in the instruments directory.
fn tone_availability() -> Vec<Availability> {
    match import::default_dir() {
        Some(dir) => TONES.iter().map(|t| t.availability(&dir)).collect(),
        None => vec![Availability::Missing(storage::NO_DATA_DIR.to_owned()); TONES.len()],
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        let ctx = ui.ctx().clone();
        self.run_held(&ctx);
        self.probe_when_idle(&ctx);
        let view = self.model.view();
        let mut command = None;
        let mut preview = None;
        let mut mark = None;
        let can_preview = !self.previewing();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Neurofeedback session");
                ui.label(egui::RichText::new(&view.connection).size(22.0).strong());
                if !view.address.is_empty() {
                    ui.label(format!("B-2: {}", view.address));
                }
                ui.horizontal(|ui| {
                    // Once a B-2 is known the status line says so; the button is only for finding one.
                    if view.address.is_empty()
                        && ui
                            .add_enabled(view.can_pair, egui::Button::new("Pair and connect"))
                            .clicked()
                    {
                        command = self.model.pair_and_connect();
                    }
                    if ui
                        // A session opens the audio output, which a playing preview holds.
                        .add_enabled(
                            view.can_start && can_preview,
                            egui::Button::new("Start session"),
                        )
                        .clicked()
                    {
                        command = self.model.start();
                    }
                    if ui
                        .add_enabled(view.can_check, egui::Button::new("Check sensors"))
                        .clicked()
                    {
                        command = self.model.check_sensors();
                    }
                    if ui
                        .add_enabled(view.checking, egui::Button::new("Stop check"))
                        .clicked()
                    {
                        command = self.model.stop_check();
                    }
                    if ui
                        .add_enabled(view.can_stop, egui::Button::new("Stop"))
                        .clicked()
                    {
                        command = self.model.stop();
                    }
                });
                ui.label(format!("Sensors: {}", view.sensor));
                egui::Grid::new("channels").show(ui, |ui| {
                    for channel in &view.channels {
                        ui.label(&channel.label);
                        ui.label(&channel.level);
                        ui.end_row();
                    }
                });
                ui.label(&view.frames);
                ui.separator();

                if view.needs_import {
                    ui.label("The instrument sounds have not been imported yet.");
                    ui.horizontal(|ui| {
                        ui.label("Original lib folder:");
                        ui.text_edit_singleline(&mut self.lib_dir);
                    });
                    if ui.button("Import instrument").clicked() {
                        command = Some(Command::Import(PathBuf::from(self.lib_dir.trim())));
                    }
                    ui.separator();
                }

                ui.label("Feedback tone");
                ui.add_enabled_ui(view.can_choose_tone, |ui| {
                    for (tone, shown) in TONES.iter().zip(&view.tones) {
                        ui.horizontal(|ui| {
                            let play = ui
                                .add_enabled(
                                    shown.available && can_preview,
                                    egui::Button::new("▶").small(),
                                )
                                .on_hover_text("Preview");
                            if play.clicked() {
                                preview = Some(tone);
                            }
                            let choice = ui.add_enabled(
                                shown.available,
                                egui::RadioButton::new(shown.selected, &shown.label),
                            );
                            if choice.clicked() {
                                self.model.select_tone(tone);
                            }
                        });
                        if !shown.available {
                            ui.small(&shown.status);
                        }
                    }
                });
                ui.separator();

                ui.add_enabled_ui(view.can_start, |ui| {
                    let p = &mut self.model.properties;
                    ui.horizontal(|ui| {
                        ui.label("Session length");
                        egui::ComboBox::from_id_salt("length")
                            .selected_text(minutes(p.duration_s))
                            .show_ui(ui, |ui| {
                                for m in SESSION_MINUTES {
                                    ui.selectable_value(&mut p.duration_s, m * 60, minutes(m * 60));
                                }
                            });
                    });
                    egui::CollapsingHeader::new("Options").show(ui, |ui| {
                        egui::Grid::new("properties").show(ui, |ui| {
                            ui.label("Interference filter");
                            egui::ComboBox::from_id_salt("interference1")
                                .selected_text(match p.interference1 {
                                    Interference1::Off => "Off",
                                    Interference1::LowPass45 => "45 Hz low-pass",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut p.interference1,
                                        Interference1::Off,
                                        "Off",
                                    );
                                    ui.selectable_value(
                                        &mut p.interference1,
                                        Interference1::LowPass45,
                                        "45 Hz low-pass",
                                    );
                                });
                            ui.end_row();
                            ui.label("Band-stop filter");
                            egui::ComboBox::from_id_salt("interference2")
                                .selected_text(match p.interference2 {
                                    Interference2::Off => "Off",
                                    Interference2::Hz25 => "25 Hz",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut p.interference2,
                                        Interference2::Off,
                                        "Off",
                                    );
                                    ui.selectable_value(
                                        &mut p.interference2,
                                        Interference2::Hz25,
                                        "25 Hz",
                                    );
                                });
                            ui.end_row();
                            ui.label("Low-pass (Hz)");
                            ui.add(egui::DragValue::new(&mut p.low_pass_hz).range(1.0..=250.0));
                            ui.end_row();
                            ui.label("High-pass (Hz)");
                            ui.add(
                                egui::DragValue::new(&mut p.high_pass_hz)
                                    .range(0.0..=50.0)
                                    .speed(0.1),
                            );
                            ui.end_row();
                        });
                    });
                });
                ui.separator();

                ui.label(&view.feedback);
                ui.label(&view.control);
                ui.label(&view.elapsed);
                ui.separator();
                ui.label(&view.notice);
                let saved = self.saved.and_then(|id| {
                    let sessions = self.history.as_ref().ok()?;
                    sessions.iter().find(|s| s.id == id)
                });
                if let Some(summary) = saved {
                    balance_grid(
                        ui,
                        ("saved balance", summary.id),
                        &history::balance(&summary.balance),
                    );
                }
                ui.separator();

                if let Some(marked) = self.past_sessions(ui) {
                    mark = Some(marked);
                }
            });
        });
        if let Some((id, test)) = mark {
            self.mark_test(id, test);
        }
        if let Some(tone) = preview {
            self.play_preview(tone, &ctx);
        }
        if let Some(command) = command {
            self.submit(command, &ctx);
        }
    }
}

/// A session's Balance Meter as a table under its note; nothing when it has none.
fn balance_grid(ui: &mut egui::Ui, id: (&str, i64), rows: &[(String, Vec<String>)]) {
    if rows.is_empty() {
        return;
    }
    ui.label(egui::RichText::new("Balance Meter").strong());
    egui::Grid::new(id).show(ui, |ui| {
        ui.label("");
        for column in history::BALANCE_COLUMNS {
            ui.label(column);
        }
        ui.end_row();
        for (meter, values) in rows {
            ui.label(meter);
            for value in values {
                ui.label(value);
            }
            ui.end_row();
        }
    });
    ui.label(history::BALANCE_NOTE);
}

/// A session length as the original lists it.
fn minutes(seconds: u32) -> String {
    format!("{} minutes", seconds / 60)
}
