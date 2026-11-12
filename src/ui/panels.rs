use super::AppState;
use super::devices::{DeviceRole, device_selector};
use super::widgets::{colored_level_meter, slider_f32};
use eframe::egui;
use std::sync::atomic::Ordering;

impl AppState {
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let running = self.engine.is_running();
                if running {
                    if ui
                        .button("Stop")
                        .on_hover_text("Stop capture, DSP, and playback streams")
                        .clicked()
                    {
                        self.stop_engine();
                    }
                } else if ui
                    .button("Start")
                    .on_hover_text(
                        "Start mic capture → filter chain → VB-Cable (and optional monitor)",
                    )
                    .clicked()
                {
                    self.start_engine();
                }

                let muted = self.params.muted.load(Ordering::Relaxed);
                if ui
                    .selectable_label(muted, if muted { "Unmute" } else { "Mute" })
                    .on_hover_text(
                        "Silence the processed output (VB-Cable and monitor). DSP still runs.",
                    )
                    .clicked()
                {
                    self.toggle_mute();
                }

                // Quick mic-test toggle in the toolbar.
                let mon = self.params.monitor_enabled.load(Ordering::Relaxed);
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
                    self.toggle_monitor();
                }

                if ui
                    .button("Save")
                    .on_hover_text("Write current settings to config.toml")
                    .clicked()
                {
                    self.persist();
                }
                if ui
                    .button("Refresh devices")
                    .on_hover_text("Re-scan Windows input/output devices (after plugging gear in)")
                    .clicked()
                {
                    self.refresh_devices();
                }
                if ui
                    .button("Quit")
                    .on_hover_text("Stop the engine, save settings, and exit")
                    .clicked()
                {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
    pub(super) fn levels_panel(&mut self, ui: &mut egui::Ui) {
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
                self.meters.input_db(),
                self.meters.input_hold_db(),
                "Level of the mic signal after mono mix, before the DSP chain.",
            );
            ui.add_space(6.0);
            colored_level_meter(
                ui,
                "Output (filtered)",
                self.meters.output_db(),
                self.meters.output_hold_db(),
                "Level after the configured filter chain and mute \
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

        let status = self.engine.status();
        ui.small(format!(
            "underruns={} overruns={}  in={} Hz  out={} Hz  mon={} Hz",
            status.underruns.load(Ordering::Relaxed),
            status.overruns.load(Ordering::Relaxed),
            status.input_rate.load(Ordering::Relaxed),
            status.output_rate.load(Ordering::Relaxed),
            status.monitor_rate.load(Ordering::Relaxed),
        ));
        if !self.status_message.is_empty() {
            ui.colored_label(egui::Color32::LIGHT_BLUE, &self.status_message);
        }
        ui.separator();
    }
    pub(super) fn monitor_panel(&mut self, ui: &mut egui::Ui) {
        let status = self.engine.status();
        // —— Mic Test / Monitor ——
        egui::CollapsingHeader::new("Mic Test (monitor filtered audio)")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(
                    "Hear the processed mic on headphones/speakers to verify noise reduction \
                         and levels. Prefer headphones to avoid feedback.",
                );

                let mut mon = self.params.monitor_enabled.load(Ordering::Relaxed);
                if ui
                    .checkbox(&mut mon, "Enable monitor (play filtered audio locally)")
                    .on_hover_text(
                        "Routes the filtered signal to a local playback device so you can \
                             hear the result without opening Discord/Zoom. Does not change \
                             VB-Cable output.",
                    )
                    .changed()
                {
                    self.set_monitor_enabled(mon);
                }

                slider_f32(
                    ui,
                    &self.params.monitor_gain_db,
                    -36.0..=12.0,
                    "Monitor volume dB",
                    "Independent listen level for mic test. Does not affect the level sent \
                         to VB-Cable or other apps.",
                );

                ui.label("Monitor playback device").on_hover_text(
                    "Headphones or speakers used for mic test. Avoid the virtual cable and \
                         speakers next to the mic (feedback).",
                );
                device_selector(
                    ui,
                    DeviceRole::Monitor,
                    &mut self.config.monitor_device,
                    &self.outputs,
                );

                if ui
                    .button("Apply monitor device (restart engine)")
                    .on_hover_text("Needed after changing the monitor device")
                    .clicked()
                {
                    self.apply_devices();
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
    }
    pub(super) fn devices_panel(&mut self, ui: &mut egui::Ui) {
        // Devices
        egui::CollapsingHeader::new("Devices")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("Input (physical mic)").on_hover_text(
                    "Hardware microphone to capture. Prefer your real mic, not CABLE Output.",
                );
                device_selector(
                    ui,
                    DeviceRole::Input,
                    &mut self.config.input_device,
                    &self.inputs,
                );

                ui.label("Output (VB-Cable Input)").on_hover_text(
                    "Where filtered audio is written. Other apps should use CABLE Output \
                         as their microphone.",
                );
                device_selector(
                    ui,
                    DeviceRole::Output,
                    &mut self.config.output_device,
                    &self.outputs,
                );

                if ui
                    .button("Apply devices (restart)")
                    .on_hover_text(
                        "Reopen audio streams with the selected input/output devices and \
                             save the config.",
                    )
                    .clicked()
                {
                    self.apply_devices();
                }
            });
    }
    pub(super) fn startup_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Startup")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut self.config.start_processing,
                    "Start processing on launch",
                )
                .on_hover_text(
                    "Automatically open the audio engine when the app starts so Discord can \
                         use CABLE Output immediately.",
                );
                ui.checkbox(&mut self.config.start_minimized, "Start minimized")
                    .on_hover_text(
                        "Hide the settings window on launch (tray icon remains available).",
                    );
            });
    }

    pub(super) fn limiter_panel(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.strong("Final-stage peak limiter");
            ui.label("Always after the filter chain, before mute. Disabled by default.");
            let before = self.config.limiter.clone();
            let limiter = &mut self.config.limiter;
            ui.checkbox(&mut limiter.enabled, "Enable sample-peak limiter");
            ui.add(egui::Slider::new(&mut limiter.ceiling_db, -24.0..=0.0).text("Ceiling dBFS"));
            ui.add(egui::Slider::new(&mut limiter.release_ms, 10.0..=1000.0).text("Release ms"));
            if *limiter != before {
                self.params.publish_chain(&self.config.filters, limiter);
            }
            let reduction = if self.engine.is_running() && limiter.enabled {
                self.params.limiter_reduction_db.load(Ordering::Relaxed)
            } else {
                0.0
            };
            ui.label(format!("Live gain reduction: {reduction:.1} dB"));
            ui.add(egui::ProgressBar::new((reduction / 24.0).clamp(0.0, 1.0)));
            if self.params.tuning.snapshot().playing {
                let replay_reduction = if limiter.enabled {
                    self.params
                        .replay_limiter_reduction_db
                        .load(Ordering::Relaxed)
                } else {
                    0.0
                };
                ui.label(format!("Replay gain reduction: {replay_reduction:.1} dB"));
            }
            ui.small(
                "Instant attack, no added buffering. Not true-peak limiting; cannot repair \
                upstream clipping or protect against later resampling / monitor-volume boosts.",
            );
        });
    }
}
