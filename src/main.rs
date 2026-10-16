//! scrcpy wireless setup — a small egui front-end for the
//! pair → find port → connect → mirror workflow.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;
mod tools;

use std::env;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use eframe::egui;
use store::Device;
use tools::{Backend, Shared};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([460.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "scrcpy wireless",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(cc.egui_ctx.clone())))),
    )
}

struct App {
    shared: std::sync::Arc<Shared>,
    ctx: egui::Context,
    dir: String,
    pair_addr: String,
    pair_code: String,
    device_ip: String,
    devices: Vec<Device>,
    selected: usize,
    save_name: String,
    save_addr: String,
}

impl App {
    fn new(ctx: egui::Context) -> Self {
        Self {
            shared: Shared::new(),
            ctx,
            dir: find_tools_dir(),
            pair_addr: String::new(),
            pair_code: String::new(),
            device_ip: String::new(),
            devices: store::load(),
            selected: 0,
            save_name: String::new(),
            save_addr: String::new(),
        }
    }

    fn persist(&self) {
        if let Err(e) = store::save(&self.devices) {
            if let Ok(mut l) = self.shared.log.lock() {
                l.push_str(&format!("could not save devices: {e}\n"));
            }
        }
    }

    fn backend(&self) -> Backend {
        Backend {
            shared: self.shared.clone(),
            dir: self.dir.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let busy = self.shared.busy.load(Ordering::SeqCst);
        let adb_ok = PathBuf::from(&self.dir).join("adb").is_file();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("scrcpy wireless setup");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("scrcpy folder:");
                ui.add(egui::TextEdit::singleline(&mut self.dir).desired_width(f32::INFINITY));
            });
            if !adb_ok {
                ui.colored_label(egui::Color32::RED, "adb not found in this folder");
            }
            ui.separator();

            ui.strong("Saved devices");
            ui.horizontal(|ui| {
                let label = self
                    .devices
                    .get(self.selected)
                    .map(|d| format!("{} ({})", d.name, d.addr))
                    .unwrap_or_else(|| "— none —".to_string());
                egui::ComboBox::from_id_salt("devices").selected_text(label).show_ui(ui, |ui| {
                    for (i, d) in self.devices.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, i, format!("{} ({})", d.name, d.addr));
                    }
                });
            });
            ui.horizontal(|ui| {
                let have = self.devices.get(self.selected).is_some();
                if ui
                    .add_enabled(!busy && adb_ok && have, egui::Button::new("⚡ Quick Connect & Mirror"))
                    .clicked()
                {
                    let addr = self.devices[self.selected].addr.clone();
                    self.backend().quick_connect(addr);
                }
                if ui.add_enabled(have, egui::Button::new("Delete")).clicked() {
                    self.devices.remove(self.selected);
                    if self.selected > 0 {
                        self.selected -= 1;
                    }
                    self.persist();
                }
            });
            // Prefill the save address from a live connection once available.
            if self.save_addr.is_empty() {
                if let Ok(t) = self.shared.mirror_target.lock() {
                    if let Some(a) = t.as_ref() {
                        self.save_addr = a.clone();
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label("name");
                ui.add(egui::TextEdit::singleline(&mut self.save_name).desired_width(90.0));
                ui.label("ip:port");
                ui.add(egui::TextEdit::singleline(&mut self.save_addr).desired_width(130.0));
                let can_save = !self.save_name.trim().is_empty() && self.save_addr.contains(':');
                if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                    self.devices.push(Device {
                        name: self.save_name.trim().to_string(),
                        addr: self.save_addr.trim().to_string(),
                    });
                    self.selected = self.devices.len() - 1;
                    self.save_name.clear();
                    self.persist();
                }
            });
            ui.separator();

            ui.strong("1. Pair");
            grid(ui, "pair", |ui| {
                ui.label("Pairing IP:port");
                ui.text_edit_singleline(&mut self.pair_addr);
                ui.end_row();
                ui.label("Pair code");
                ui.text_edit_singleline(&mut self.pair_code);
                ui.end_row();
            });
            if ui.add_enabled(!busy && adb_ok, egui::Button::new("Pair")).clicked() {
                if self.device_ip.is_empty() {
                    self.device_ip = host_of(&self.pair_addr);
                }
                self.backend().pair(self.pair_addr.trim().to_string(), self.pair_code.trim().to_string());
            }
            ui.separator();

            ui.strong("2. Find port & connect");
            grid(ui, "conn", |ui| {
                ui.label("Device IP");
                ui.text_edit_singleline(&mut self.device_ip);
                ui.end_row();
            });
            if ui.add_enabled(!busy && adb_ok, egui::Button::new("Find port & Connect")).clicked() {
                let ip = if self.device_ip.trim().is_empty() {
                    host_of(&self.pair_addr)
                } else {
                    self.device_ip.trim().to_string()
                };
                self.backend().find_and_connect(ip);
            }
            ui.separator();

            ui.strong("3. Mirror");
            if ui.add_enabled(!busy && adb_ok, egui::Button::new("▶ Start Mirror")).clicked() {
                self.backend().start_mirror();
            }
            ui.separator();

            ui.horizontal(|ui| {
                if busy {
                    ui.spinner();
                    ui.label("working…");
                } else {
                    ui.label("idle");
                }
                ui.separator();
                ui.label(check("paired", self.shared.paired.load(Ordering::SeqCst)));
                ui.label(check("connected", self.shared.connected.load(Ordering::SeqCst)));
            });

            ui.add_space(4.0);
            ui.label("Log:");
            let log = self.shared.log.lock().map(|l| l.clone()).unwrap_or_default();
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut log.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10),
                );
            });
        });
    }
}

fn grid(ui: &mut egui::Ui, id: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 6.0]).show(ui, body);
}

fn check(label: &str, on: bool) -> String {
    format!("{} {}", if on { "✓" } else { "·" }, label)
}

/// "192.168.1.136:33197" -> "192.168.1.136"
fn host_of(addr: &str) -> String {
    addr.trim().rsplit_once(':').map(|(h, _)| h).unwrap_or(addr.trim()).to_string()
}

/// Locate the folder holding `adb`/`scrcpy`: $SCRCPY_DIR, then two levels up
/// from the exe (…/scrcpy-wireless-gui/target/release/exe), then the cwd.
fn find_tools_dir() -> String {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(d) = env::var("SCRCPY_DIR") {
        candidates.push(PathBuf::from(d));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(rel) = exe.ancestors().nth(4) {
            candidates.push(rel.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd);
    }
    for c in candidates {
        if c.join("adb").is_file() {
            return c.to_string_lossy().into_owned();
        }
    }
    String::new()
}
