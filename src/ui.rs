use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui;
use parking_lot::Mutex;

use crate::audio::{list_input_devices, list_output_devices, AudioEngine, DeviceInfo, LevelMeters};
use crate::config::{save_config, AppConfig, LiveParams};

pub struct AppState {
    pub config: AppConfig,
    pub params: Arc<LiveParams>,
    pub engine: Arc<AudioEngine>,
    pub meters: Arc<LevelMeters>,
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
    pub status_message: String,
    pub quit_requested: Arc<Mutex<bool>>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        params: Arc<LiveParams>,
        engine: Arc<AudioEngine>,
        meters: Arc<LevelMeters>,
        quit_requested: Arc<Mutex<bool>>,
    ) -> Self {
        let inputs = list_input_devices().unwrap_or_default();
        let outputs = list_output_devices().unwrap_or_default();
        Self {
            config,
            params,
            engine,
            meters,
            inputs,
            outputs,
            status_message: String::new(),
            quit_requested,
        }
    }

    pub fn refresh_devices(&mut self) {
        self.inputs = list_input_devices().unwrap_or_default();
        self.outputs = list_output_devices().unwrap_or_default();
    }

    pub fn start_engine(&mut self) {
        let input = self.config.input_device.as_deref();
        let output = self.config.output_device.as_deref();
        match self.engine.start(input, output) {
            Ok(()) => {
                self.status_message = format!(
                    "Running: {} → {}",
                    self.engine.status().input_name.lock().clone(),
                    self.engine.status().output_name.lock().clone()
                );
            }
            Err(err) => {
                self.status_message = format!("Start failed: {err:#}");
                tracing::error!(%err, "engine start failed");
            }
        }
    }

    pub fn stop_engine(&mut self) {
        self.engine.stop();
        self.status_message = "Stopped".into();
    }

    pub fn persist(&mut self) {
        self.params.snapshot_into(&mut self.config);
        if let Err(err) = save_config(&self.config) {
            self.status_message = format!("Save failed: {err:#}");
        } else {
            self.status_message = "Settings saved".into();
        }
    }
}

pub struct MicProcessorApp {
    pub state: AppState,
}

impl eframe::App for MicProcessorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep UI responsive for meters.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        self.state.meters.decay();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let running = self.state.engine.is_running();
                if running {
                    if ui.button("Stop").clicked() {
                        self.state.stop_engine();
                    }
                } else if ui.button("Start").clicked() {
                    self.state.start_engine();
                }

                let muted = self.state.params.muted.load(Ordering::Relaxed);
                if ui
                    .selectable_label(muted, if muted { "Unmute" } else { "Mute" })
                    .clicked()
                {
                    self.state.params.muted.store(!muted, Ordering::Relaxed);
                }

                if ui.button("Save").clicked() {
                    self.state.persist();
                }
                if ui.button("Refresh devices").clicked() {
                    self.state.refresh_devices();
                }
                if ui.button("Quit").clicked() {
                    *self.state.quit_requested.lock() = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Mic Processor");
            ui.label("Noise Suppression (RNNoise) → Expander → Compressor → Virtual Cable");
            ui.separator();

            ui.horizontal(|ui| {
                ui.group(|ui| {
                    ui.label("Input peak");
                    let db = self.state.meters.input_db();
                    ui.add(egui::ProgressBar::new(((db + 80.0) / 80.0).clamp(0.0, 1.0)).text(format!("{db:.1} dB")));
                });
                ui.group(|ui| {
                    ui.label("Output peak");
                    let db = self.state.meters.output_db();
                    ui.add(egui::ProgressBar::new(((db + 80.0) / 80.0).clamp(0.0, 1.0)).text(format!("{db:.1} dB")));
                });
            });

            let status = self.state.engine.status();
            ui.small(format!(
                "underruns={} overruns={}  in={} Hz  out={} Hz",
                status.underruns.load(Ordering::Relaxed),
                status.overruns.load(Ordering::Relaxed),
                status.input_rate.load(Ordering::Relaxed),
                status.output_rate.load(Ordering::Relaxed),
            ));
            if !self.state.status_message.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_BLUE, &self.state.status_message);
            }
            ui.separator();

            // Devices
            egui::CollapsingHeader::new("Devices")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("Input (physical mic)");
                    let mut input_sel = self
                        .state
                        .config
                        .input_device
                        .clone()
                        .unwrap_or_else(|| "(default)".into());
                    egui::ComboBox::from_id_salt("input_dev")
                        .selected_text(&input_sel)
                        .width(360.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut input_sel, "(default)".into(), "(system default)")
                                .clicked()
                            {
                                self.state.config.input_device = None;
                            }
                            for d in &self.state.inputs {
                                let label = if d.is_default {
                                    format!("{} (default)", d.name)
                                } else {
                                    d.name.clone()
                                };
                                if ui
                                    .selectable_value(&mut input_sel, d.name.clone(), label)
                                    .clicked()
                                {
                                    self.state.config.input_device = Some(d.name.clone());
                                }
                            }
                        });

                    ui.label("Output (VB-Cable Input)");
                    let mut output_sel = self
                        .state
                        .config
                        .output_device
                        .clone()
                        .unwrap_or_else(|| "(auto VB-Cable / default)".into());
                    egui::ComboBox::from_id_salt("output_dev")
                        .selected_text(&output_sel)
                        .width(360.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut output_sel,
                                    "(auto VB-Cable / default)".into(),
                                    "(auto VB-Cable / default)",
                                )
                                .clicked()
                            {
                                self.state.config.output_device = None;
                            }
                            for d in &self.state.outputs {
                                let label = if d.looks_like_vb_cable {
                                    format!("{} ★", d.name)
                                } else if d.is_default {
                                    format!("{} (default)", d.name)
                                } else {
                                    d.name.clone()
                                };
                                if ui
                                    .selectable_value(&mut output_sel, d.name.clone(), label)
                                    .clicked()
                                {
                                    self.state.config.output_device = Some(d.name.clone());
                                }
                            }
                        });

                    if ui.button("Apply devices (restart)").clicked() {
                        if self.state.engine.is_running() {
                            self.state.start_engine();
                        }
                        self.state.persist();
                    }
                });

            // High-pass + RNNoise
            egui::CollapsingHeader::new("Pre-processing")
                .default_open(true)
                .show(ui, |ui| {
                    let mut hp = self.state.params.highpass_enabled.load(Ordering::Relaxed);
                    if ui.checkbox(&mut hp, "High-pass filter").changed() {
                        self.state.params.highpass_enabled.store(hp, Ordering::Relaxed);
                    }
                    let mut hp_hz = self.state.params.highpass_hz.load(Ordering::Relaxed);
                    if ui
                        .add(egui::Slider::new(&mut hp_hz, 40.0..=200.0).text("HPF Hz"))
                        .changed()
                    {
                        self.state.params.highpass_hz.store(hp_hz, Ordering::Relaxed);
                    }

                    let mut rn = self.state.params.rnnoise_enabled.load(Ordering::Relaxed);
                    if ui.checkbox(&mut rn, "RNNoise noise suppression").changed() {
                        self.state.params.rnnoise_enabled.store(rn, Ordering::Relaxed);
                    }
                });

            // Expander
            egui::CollapsingHeader::new("Expander")
                .default_open(true)
                .show(ui, |ui| {
                    let mut en = self.state.params.expander_enabled.load(Ordering::Relaxed);
                    if ui.checkbox(&mut en, "Enabled").changed() {
                        self.state
                            .params
                            .expander_enabled
                            .store(en, Ordering::Relaxed);
                    }
                    slider_f32(ui, &self.state.params.expander_threshold_db, -80.0..=0.0, "Threshold dB");
                    slider_f32(ui, &self.state.params.expander_ratio, 1.0..=10.0, "Ratio");
                    slider_f32(ui, &self.state.params.expander_attack_ms, 0.1..=50.0, "Attack ms");
                    slider_f32(ui, &self.state.params.expander_release_ms, 10.0..=500.0, "Release ms");
                    slider_f32(ui, &self.state.params.expander_hold_ms, 0.0..=300.0, "Hold ms");
                    slider_f32(ui, &self.state.params.expander_range_db, 0.0..=60.0, "Range dB");
                });

            // Compressor
            egui::CollapsingHeader::new("Compressor")
                .default_open(true)
                .show(ui, |ui| {
                    let mut en = self.state.params.compressor_enabled.load(Ordering::Relaxed);
                    if ui.checkbox(&mut en, "Enabled").changed() {
                        self.state
                            .params
                            .compressor_enabled
                            .store(en, Ordering::Relaxed);
                    }
                    slider_f32(ui, &self.state.params.compressor_threshold_db, -60.0..=0.0, "Threshold dB");
                    slider_f32(ui, &self.state.params.compressor_ratio, 1.0..=20.0, "Ratio");
                    slider_f32(ui, &self.state.params.compressor_attack_ms, 0.1..=100.0, "Attack ms");
                    slider_f32(ui, &self.state.params.compressor_release_ms, 10.0..=500.0, "Release ms");
                    slider_f32(ui, &self.state.params.compressor_knee_db, 0.0..=24.0, "Knee dB");
                    slider_f32(ui, &self.state.params.compressor_makeup_db, -12.0..=24.0, "Makeup dB");
                });

            // Master
            egui::CollapsingHeader::new("Master")
                .default_open(true)
                .show(ui, |ui| {
                    slider_f32(ui, &self.state.params.master_gain_db, -24.0..=24.0, "Output gain dB");
                    ui.checkbox(&mut self.state.config.start_processing, "Start processing on launch");
                    ui.checkbox(&mut self.state.config.start_minimized, "Start minimized");
                });

            ui.separator();
            ui.small("Tip: Install VB-Cable, set this app's output to \"CABLE Input\", and set apps (Discord/Zoom) to use \"CABLE Output\" as their microphone.");
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.params.snapshot_into(&mut self.state.config);
        let _ = save_config(&self.state.config);
        self.state.engine.stop();
    }
}

fn slider_f32(
    ui: &mut egui::Ui,
    value: &atomic_float::AtomicF32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
) {
    let mut v = value.load(Ordering::Relaxed);
    if ui.add(egui::Slider::new(&mut v, range).text(label)).changed() {
        value.store(v, Ordering::Relaxed);
    }
}

/// Tiny generated tray icon (16x16 teal circle).
pub fn make_tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    let size = 16u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let cx = 7.5f32;
    let cy = 7.5f32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let i = ((y * size + x) * 4) as usize;
            if dx * dx + dy * dy <= 6.5 * 6.5 {
                rgba[i] = 0x2E;
                rgba[i + 1] = 0xCC;
                rgba[i + 2] = 0xB0;
                rgba[i + 3] = 0xFF;
            }
        }
    }
    (rgba, size, size)
}