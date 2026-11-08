//! The egui window. It draws [`UiModel`], turns widget input into model calls and
//! [`Command`]s, and carries out the import and the tone check by calling
//! `import::import_banks` and `Tone::availability` (tested in their own modules).

use std::path::PathBuf;

use eframe::egui;

use crate::audio::import;
use crate::audio::tone::{self, Availability, TONES};
use crate::feedback::{Interference1, Interference2};
use crate::storage;

use super::bluetooth::BluetoothHost;
use super::bluez::Bluez;
use super::state::{Command, Event, UiModel};
use super::worker::Worker;

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
    lib_dir: String,
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
            lib_dir: String::new(),
        }
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
        let Some(worker) = &self.worker else { return };
        while let Ok(event) = worker.events.try_recv() {
            self.model.apply(event);
        }
    }
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
        let view = self.model.view();
        let mut command = None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Neurofeedback session");
            if view.address.is_empty() {
                ui.label("B-2: not paired yet");
            } else {
                ui.label(format!("B-2: {}", view.address));
            }
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
                    let choice = ui.add_enabled(
                        shown.available,
                        egui::RadioButton::new(shown.selected, &shown.label),
                    );
                    if choice.clicked() {
                        self.model.select_tone(tone);
                    }
                    if !shown.available {
                        ui.small(&shown.status);
                    }
                }
            });
            ui.separator();

            ui.add_enabled_ui(view.can_start, |ui| {
                let p = &mut self.model.properties;
                egui::Grid::new("properties").show(ui, |ui| {
                    ui.label("Session length (s)");
                    ui.add(egui::DragValue::new(&mut p.duration_s).range(1..=3600));
                    ui.end_row();
                    ui.label("Interference filter");
                    egui::ComboBox::from_id_salt("interference1")
                        .selected_text(match p.interference1 {
                            Interference1::Off => "Off",
                            Interference1::LowPass45 => "45 Hz low-pass",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut p.interference1, Interference1::Off, "Off");
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
                            ui.selectable_value(&mut p.interference2, Interference2::Off, "Off");
                            ui.selectable_value(&mut p.interference2, Interference2::Hz25, "25 Hz");
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
            ui.separator();

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(view.can_pair, egui::Button::new("Pair and connect"))
                    .clicked()
                {
                    command = self.model.pair_and_connect();
                }
                if ui
                    .add_enabled(view.can_start, egui::Button::new("Start session"))
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
            ui.separator();

            ui.label(&view.connection);
            ui.label(format!("Sensors: {}", view.sensor));
            egui::Grid::new("channels").show(ui, |ui| {
                for channel in &view.channels {
                    ui.label(&channel.label);
                    ui.label(&channel.level);
                    ui.end_row();
                }
            });
            ui.label(&view.frames);
            ui.label(&view.feedback);
            ui.label(&view.control);
            ui.label(&view.elapsed);
            ui.separator();
            ui.label(&view.notice);
        });
        if let Some(command) = command {
            self.run(command, &ctx);
        }
    }
}
