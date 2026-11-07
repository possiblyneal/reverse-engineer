//! The egui window. It draws [`UiModel`], turns widget input into model calls and
//! [`Command`]s, and carries out the import and the instrument check by calling
//! `import::import_banks` and `audio::load_instrument` (tested in their own modules).

use std::path::PathBuf;

use eframe::egui;

use crate::audio::{self, import};
use crate::feedback::{Interference1, Interference2};
use crate::live::INSTRUMENT_ID;
use crate::storage;

use super::state::{Command, Event, Instrument, UiModel};
use super::worker::Worker;

/// Open the window for the B-2 at `mac` and run until it is closed.
///
/// # Errors
/// Fails if the window cannot be created.
pub fn run(mac: String) -> eframe::Result {
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
    mac: String,
    model: UiModel,
    worker: Option<Worker>,
    lib_dir: String,
}

impl App {
    fn new(mac: String) -> Self {
        let model = UiModel::new(mac.clone(), instrument());
        Self {
            mac,
            model,
            worker: None,
            lib_dir: String::new(),
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Start(properties) => {
                let ctx = ctx.clone();
                self.worker = Some(Worker::spawn(self.mac.clone(), properties, move || {
                    ctx.request_repaint();
                }));
            }
            Command::CheckSensors => {
                let ctx = ctx.clone();
                self.worker = Some(Worker::spawn_check(
                    self.mac.clone(),
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

fn instrument() -> Instrument {
    let loaded = import::default_dir()
        .ok_or_else(|| storage::NO_DATA_DIR.to_owned())
        .and_then(|dir| audio::load_instrument(&dir, INSTRUMENT_ID).map_err(|e| e.to_string()));
    match loaded {
        Ok(_) => Instrument::Ready,
        Err(why) => Instrument::Missing(why),
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
            ui.label(format!("B-2: {}", self.mac));
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
            ui.label(&view.elapsed);
            ui.separator();
            ui.label(&view.notice);
        });
        if let Some(command) = command {
            self.run(command, &ctx);
        }
    }
}
