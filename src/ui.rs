use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui;
use parking_lot::Mutex;

use crate::audio::{
    db_to_meter, list_input_devices, list_output_devices, AudioEngine, DeviceInfo, LevelMeters,
};
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
        let monitor = self.config.monitor_device.as_deref();
        match self.engine.start(input, output, monitor) {
            Ok(()) => {
                let st = self.engine.status();
                let mon = st.monitor_name.lock().clone();
                self.status_message = if mon.is_empty() {
                    format!(
                        "Running: {} → {} (no monitor device)",
                        st.input_name.lock(),
                        st.output_name.lock()
                    )
                } else {
                    format!(
                        "Running: {} → {} | monitor: {}",
                        st.input_name.lock(),
                        st.output_name.lock(),
                        mon
                    )
                };
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
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        self.state.meters.decay();

egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let running = self.state.engine.is_running();
                if running {
                    if ui
                        .button("Stop")
                        .on_hover_text("Stop capture, DSP, and playback streams")
                        .clicked()
                    {
                        self.state.stop_engine();
                    }
                } else if ui
                    .button("Start")
                    .on_hover_text(
                        "Start mic capture → filter chain → VB-Cable (and optional monitor)",
                    )
                    .clicked()
                {
                    self.state.start_engine();
                }

                let muted = self.state.params.muted.load(Ordering::Relaxed);
                if ui
                    .selectable_label(muted, if muted { "Unmute" } else { "Mute" })
                    .on_hover_text(
                        "Silence the processed output (VB-Cable and monitor). DSP still runs.",
                    )
                    .clicked()
                {
                    self.state.params.muted.store(!muted, Ordering::Relaxed);
                }

                // Quick mic-test toggle in the toolbar.
                let mon = self.state.params.monitor_enabled.load(Ordering::Relaxed);
                if ui
                    .selectable_label(
                        mon,
                        if mon {
                            "🎙 Mic Test ON"
                        } else {
                            "🎙 Mic Test"
                        },
                    )
                    .on_hover_text(
                        "Play the filtered mic to headphones/speakers so you can hear the chain. \
                         Prefer headphones to avoid feedback.",
                    )
                    .clicked()
                {
                    self.state
                        .params
                        .monitor_enabled
                        .store(!mon, Ordering::Relaxed);
                    self.state.config.monitor_enabled = !mon;
                }

                if ui
                    .button("Save")
                    .on_hover_text("Write current settings to config.toml")
                    .clicked()
                {
                    self.state.persist();
                }
                if ui
                    .button("Refresh devices")
                    .on_hover_text("Re-scan Windows input/output devices (after plugging gear in)")
                    .clicked()
                {
                    self.state.refresh_devices();
                }
                if ui
                    .button("Quit")
                    .on_hover_text("Stop the engine, save settings, and exit")
                    .clicked()
                {
                    *self.state.quit_requested.lock() = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            ui.heading("Mic Processor");
            ui.label("Noise Suppression (RNNoise) → Expander → Compressor → Virtual Cable");
            ui.separator();

// —— Level meters (green / yellow / red) ——
            ui.group(|ui| {
                ui.label(egui::RichText::new("Levels").strong())
                    .on_hover_text(
                        "Peak meters in dBFS. Green is safe, yellow is hot, red is near clipping. \
                         White tick is peak hold.",
                    );
                colored_level_meter(
                    ui,
                    "Input",
                    self.state.meters.input_db(),
                    self.state.meters.input_hold_db(),
                    "Level of the mic signal after mono mix, before the DSP chain.",
                );
                ui.add_space(6.0);
                colored_level_meter(
                    ui,
                    "Output (filtered)",
                    self.state.meters.output_db(),
                    self.state.meters.output_hold_db(),
                    "Level after HPF → RNNoise → expander → compressor → master gain \
                     (what goes to VB-Cable / monitor).",
                );
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(40, 180, 70), "■ green < −12 dB")
                        .on_hover_text("Comfortable headroom");
                    ui.colored_label(egui::Color32::from_rgb(220, 180, 30), "■ yellow −12…−6")
                        .on_hover_text("Getting loud — watch peaks");
                    ui.colored_label(egui::Color32::from_rgb(220, 50, 50), "■ red > −6 dB")
                        .on_hover_text("Near digital full scale — risk of clipping");
                });
            });

            let status = self.state.engine.status();
            ui.small(format!(
                "underruns={} overruns={}  in={} Hz  out={} Hz  mon={} Hz",
                status.underruns.load(Ordering::Relaxed),
                status.overruns.load(Ordering::Relaxed),
                status.input_rate.load(Ordering::Relaxed),
                status.output_rate.load(Ordering::Relaxed),
                status.monitor_rate.load(Ordering::Relaxed),
            ));
            if !self.state.status_message.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_BLUE, &self.state.status_message);
            }
            ui.separator();

            // —— Mic Test / Monitor ——
            egui::CollapsingHeader::new("Mic Test (monitor filtered audio)")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        "Hear the processed mic on headphones/speakers to verify noise reduction \
                         and levels. Prefer headphones to avoid feedback.",
                    );

let mut mon = self.state.params.monitor_enabled.load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut mon, "Enable monitor (play filtered audio locally)")
                        .on_hover_text(
                            "Routes the filtered signal to a local playback device so you can \
                             hear the result without opening Discord/Zoom. Does not change \
                             VB-Cable output.",
                        )
                        .changed()
                    {
                        self.state
                            .params
                            .monitor_enabled
                            .store(mon, Ordering::Relaxed);
                        self.state.config.monitor_enabled = mon;
                    }

                    slider_f32(
                        ui,
                        &self.state.params.monitor_gain_db,
                        -36.0..=12.0,
                        "Monitor volume dB",
                        "Independent listen level for mic test. Does not affect the level sent \
                         to VB-Cable or other apps.",
                    );

                    ui.label("Monitor playback device").on_hover_text(
                        "Headphones or speakers used for mic test. Avoid the virtual cable and \
                         speakers next to the mic (feedback).",
                    );
                    let mut mon_sel = self
                        .state
                        .config
                        .monitor_device
                        .clone()
                        .unwrap_or_else(|| "(auto headphones / default)".into());
                    egui::ComboBox::from_id_salt("monitor_dev")
                        .selected_text(&mon_sel)
                        .width(360.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut mon_sel,
                                    "(auto headphones / default)".into(),
                                    "(auto headphones / default)",
                                )
                                .clicked()
                            {
                                self.state.config.monitor_device = None;
                            }
                            for d in &self.state.outputs {
                                // Skip pure virtual-cable targets in the monitor list when possible.
                                let label = if d.is_default {
                                    format!("{} (default)", d.name)
                                } else if d.looks_like_vb_cable {
                                    format!("{} (virtual)", d.name)
                                } else {
                                    d.name.clone()
                                };
                                if ui
                                    .selectable_value(&mut mon_sel, d.name.clone(), label)
                                    .clicked()
                                {
                                    self.state.config.monitor_device = Some(d.name.clone());
                                }
                            }
                        });

                    if ui
                        .button("Apply monitor device (restart engine)")
                        .on_hover_text("Needed after changing the monitor device")
                        .clicked()
                    {
                        if self.state.engine.is_running() {
                            self.state.start_engine();
                        }
                        self.state.persist();
                    }

                    let mon_name = status.monitor_name.lock().clone();
                    if mon_name.is_empty() {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "No monitor device open — pick speakers/headphones and Apply.",
                        );
                    } else {
                        ui.small(format!("Active monitor stream: {mon_name}"));
                    }
                });

            // Devices
            egui::CollapsingHeader::new("Devices")
                .default_open(true)
                .show(ui, |ui| {
ui.label("Input (physical mic)").on_hover_text(
                        "Hardware microphone to capture. Prefer your real mic, not CABLE Output.",
                    );
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
                                .selectable_value(
                                    &mut input_sel,
                                    "(default)".into(),
                                    "(system default)",
                                )
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

ui.label("Output (VB-Cable Input)").on_hover_text(
                        "Where filtered audio is written. Other apps should use CABLE Output \
                         as their microphone.",
                    );
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

if ui
                        .button("Apply devices (restart)")
                        .on_hover_text(
                            "Reopen audio streams with the selected input/output devices and \
                             save the config.",
                        )
                        .clicked()
                    {
                        if self.state.engine.is_running() {
                            self.state.start_engine();
                        }
                        self.state.persist();
                    }
                });

            egui::CollapsingHeader::new("Pre-processing")
                .default_open(true)
                .show(ui, |ui| {
                    let mut hp = self.state.params.highpass_enabled.load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut hp, "High-pass filter")
                        .on_hover_text(
                            "Removes low-frequency rumble (desk bumps, HVAC) before RNNoise. \
                             Usually leave on around 80 Hz.",
                        )
                        .changed()
                    {
                        self.state
                            .params
                            .highpass_enabled
                            .store(hp, Ordering::Relaxed);
                    }
                    let mut hp_hz = self.state.params.highpass_hz.load(Ordering::Relaxed);
                    if ui
                        .add(egui::Slider::new(&mut hp_hz, 40.0..=200.0).text("HPF Hz"))
                        .on_hover_text(
                            "Cutoff frequency of the rumble filter. Higher = thinner low end; \
                             lower = more bass (and rumble).",
                        )
                        .changed()
                    {
                        self.state.params.highpass_hz.store(hp_hz, Ordering::Relaxed);
                    }

                    let mut rn = self.state.params.rnnoise_enabled.load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut rn, "RNNoise noise suppression")
                        .on_hover_text(
                            "Neural noise suppressor (nnnoiseless). Cuts steady background \
                             noise while preserving speech. Adds ~10 ms latency per frame.",
                        )
                        .changed()
                    {
                        self.state
                            .params
                            .rnnoise_enabled
                            .store(rn, Ordering::Relaxed);
                    }
                });

egui::CollapsingHeader::new("Expander")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Downward expander / soft gate — quiets residual noise in pauses",
                        )
                        .small()
                        .weak(),
                    );
                    let mut en = self.state.params.expander_enabled.load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut en, "Enabled")
                        .on_hover_text(
                            "When the signal is below the threshold, gain is reduced to hush \
                             leftover noise after RNNoise.",
                        )
                        .changed()
                    {
                        self.state
                            .params
                            .expander_enabled
                            .store(en, Ordering::Relaxed);
                    }
                    slider_f32(
                        ui,
                        &self.state.params.expander_threshold_db,
                        -80.0..=0.0,
                        "Threshold dB",
                        "Level below which expansion starts. Raise if quiet speech is cut; \
                         lower if noise still leaks through.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.expander_ratio,
                        1.0..=10.0,
                        "Ratio",
                        "How hard quiet signals are pushed down. 1 = off; higher = stronger gate.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.expander_attack_ms,
                        0.1..=50.0,
                        "Attack ms",
                        "How fast gain drops when the signal goes below threshold. \
                         Faster = snappier gate; slower = less choppy.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.expander_release_ms,
                        10.0..=500.0,
                        "Release ms",
                        "How fast gain recovers when speech returns. Too fast can sound pump-y.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.expander_hold_ms,
                        0.0..=300.0,
                        "Hold ms",
                        "Keeps the gate open briefly after speech so word endings are not clipped.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.expander_range_db,
                        0.0..=60.0,
                        "Range dB",
                        "Maximum attenuation applied in silence. Higher = quieter pauses.",
                    );
                });

egui::CollapsingHeader::new("Compressor")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Soft-knee compressor — even out loud and quiet speech",
                        )
                        .small()
                        .weak(),
                    );
                    let mut en = self
                        .state
                        .params
                        .compressor_enabled
                        .load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut en, "Enabled")
                        .on_hover_text(
                            "Reduces dynamic range so soft speech is closer to loud speech \
                             without harsh clipping.",
                        )
                        .changed()
                    {
                        self.state
                            .params
                            .compressor_enabled
                            .store(en, Ordering::Relaxed);
                    }
                    slider_f32(
                        ui,
                        &self.state.params.compressor_threshold_db,
                        -60.0..=0.0,
                        "Threshold dB",
                        "Level above which compression engages. Lower = more of the signal is \
                         compressed.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.compressor_ratio,
                        1.0..=20.0,
                        "Ratio",
                        "Compression strength (e.g. 3:1). Higher = more leveling; very high \
                         approaches limiting.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.compressor_attack_ms,
                        0.1..=100.0,
                        "Attack ms",
                        "How quickly gain reduction starts on loud peaks. Fast = tighter control; \
                         slow = more transient punch.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.compressor_release_ms,
                        10.0..=500.0,
                        "Release ms",
                        "How quickly gain returns after a loud phrase. Match roughly to speech \
                         pace to avoid pumping.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.compressor_knee_db,
                        0.0..=24.0,
                        "Knee dB",
                        "Soft-knee width. Wider = smoother, less obvious transition into \
                         compression.",
                    );
                    slider_f32(
                        ui,
                        &self.state.params.compressor_makeup_db,
                        -12.0..=24.0,
                        "Makeup dB",
                        "Gain added after compression to restore overall loudness.",
                    );
                });

            egui::CollapsingHeader::new("Master")
                .default_open(true)
                .show(ui, |ui| {
                    slider_f32(
                        ui,
                        &self.state.params.master_gain_db,
                        -24.0..=24.0,
                        "Output gain dB",
                        "Final trim on the filtered signal sent to VB-Cable (and shown on the \
                         output meter). Independent of monitor volume.",
                    );
                    ui.checkbox(
                        &mut self.state.config.start_processing,
                        "Start processing on launch",
                    )
                    .on_hover_text(
                        "Automatically open the audio engine when the app starts so Discord can \
                         use CABLE Output immediately.",
                    );
                    ui.checkbox(&mut self.state.config.start_minimized, "Start minimized")
                        .on_hover_text(
                            "Hide the settings window on launch (tray icon remains available).",
                        );
                });

ui.separator();
            ui.small(
                "Tip: Install VB-Cable, set this app's output to \"CABLE Input\", and set apps \
                 (Discord/Zoom) to use \"CABLE Output\" as their microphone. Use Mic Test with \
                 headphones to verify the chain.",
            );
            ui.add_space(12.0);
                }); // ScrollArea
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
    tip: &str,
) {
    let mut v = value.load(Ordering::Relaxed);
    if ui
        .add(egui::Slider::new(&mut v, range).text(label))
        .on_hover_text(tip)
        .changed()
    {
        value.store(v, Ordering::Relaxed);
    }
}

/// DAW-style horizontal meter: green / yellow / red zones with peak hold.
/// Label and dB columns use fixed widths so stacked meters share the same bar origin.
fn colored_level_meter(ui: &mut egui::Ui, label: &str, level_db: f32, hold_db: f32, tip: &str) {
    // Wide enough for "Output (filtered)"; keep identical across rows.
    const LABEL_W: f32 = 128.0;
    const DB_W: f32 = 72.0;
    const BAR_H: f32 = 22.0;
    const ROW_SPACING: f32 = 8.0;

ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ROW_SPACING;

        // Fixed-width label column (left-aligned text inside).
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_W, BAR_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(LABEL_W);
                ui.add(
                    egui::Label::new(egui::RichText::new(label).strong())
                        .truncate()
                        .sense(egui::Sense::hover()),
                )
                .on_hover_text(tip);
            },
        );

        // Bar width = remaining space minus fixed dB column (and spacing).
        let bar_w = (ui.available_width() - DB_W - ROW_SPACING).clamp(160.0, 420.0);
        let (rect, bar_resp) =
            ui.allocate_exact_size(egui::vec2(bar_w, BAR_H), egui::Sense::hover());
        bar_resp.on_hover_text(tip);
        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(28, 28, 32));

        // Zone backgrounds (always visible, dimmed).
        let green_end = db_to_meter(-12.0);
        let yellow_end = db_to_meter(-6.0);
        paint_zone(
            &painter,
            rect,
            0.0,
            green_end,
            egui::Color32::from_rgba_unmultiplied(30, 90, 40, 90),
        );
        paint_zone(
            &painter,
            rect,
            green_end,
            yellow_end,
            egui::Color32::from_rgba_unmultiplied(120, 100, 20, 90),
        );
        paint_zone(
            &painter,
            rect,
            yellow_end,
            1.0,
            egui::Color32::from_rgba_unmultiplied(120, 30, 30, 90),
        );

        // Active level fill with matching colors.
        let level_t = db_to_meter(level_db);
        if level_t > 0.001 {
            let g = level_t.min(green_end);
            if g > 0.0 {
                paint_zone(
                    &painter,
                    rect,
                    0.0,
                    g,
                    egui::Color32::from_rgb(45, 200, 75),
                );
            }
            if level_t > green_end {
                let y = level_t.min(yellow_end);
                paint_zone(
                    &painter,
                    rect,
                    green_end,
                    y,
                    egui::Color32::from_rgb(230, 190, 40),
                );
            }
            if level_t > yellow_end {
                paint_zone(
                    &painter,
                    rect,
                    yellow_end,
                    level_t,
                    egui::Color32::from_rgb(230, 55, 55),
                );
            }
        }

        // Peak hold tick.
        let hold_t = db_to_meter(hold_db);
        if hold_t > 0.01 {
            let x = rect.left() + rect.width() * hold_t;
            painter.line_segment(
                [
                    egui::pos2(x, rect.top() + 2.0),
                    egui::pos2(x, rect.bottom() - 2.0),
                ],
                egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
            );
        }

        // Border
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(70)),
        );

        // Fixed-width dB readout so bar ends align across rows.
        ui.allocate_ui_with_layout(
            egui::vec2(DB_W, BAR_H),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_width(DB_W);
                ui.monospace(format!("{level_db:5.1} dB"));
            },
        );
    });
}

fn paint_zone(
    painter: &egui::Painter,
    full: egui::Rect,
    t0: f32,
    t1: f32,
    color: egui::Color32,
) {
    if t1 <= t0 {
        return;
    }
    let left = full.left() + full.width() * t0;
    let right = full.left() + full.width() * t1;
    let r = egui::Rect::from_min_max(
        egui::pos2(left, full.top() + 2.0),
        egui::pos2(right, full.bottom() - 2.0),
    );
    painter.rect_filled(r, 2.0, color);
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
