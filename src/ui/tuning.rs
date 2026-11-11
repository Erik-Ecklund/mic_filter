use std::sync::atomic::Ordering;

use eframe::egui;

use super::AppState;
use crate::audio::{Comparison, MAX_RECORD_SECONDS};

impl AppState {
    pub(super) fn tuning_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Record & replay tuning")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(format!(
                    "Record up to {MAX_RECORD_SECONDS} seconds of dry mic audio, then loop it \
                     while adjusting filters. Clips stay in memory only."
                ));
                ui.label(
                    "Headphones only: replay replaces Mic Test audio, never the cable output. \
                     The cable continues transmitting your LIVE mic.",
                );
                let snapshot = self.params.tuning.snapshot();
                let running = self.engine.is_running();
                let monitor_ready =
                    self.engine.status().monitor_rate.load(Ordering::Relaxed) != 0;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            running && !snapshot.recording,
                            egui::Button::new(if snapshot.seconds > 0.0 {
                                "Record new clip"
                            } else {
                                "Record dry mic"
                            }),
                        )
                        .on_hover_text("Replaces the previous clip. Recording is before all filters and mute.")
                        .clicked()
                    {
                        self.params.tuning.record();
                    }
                    if ui
                        .add_enabled(
                            snapshot.recording || snapshot.playing,
                            egui::Button::new("Stop recording / replay"),
                        )
                        .on_hover_text("Retains the clip. Stopping replay returns Mic Test to the live mic.")
                        .clicked()
                    {
                        self.params.tuning.stop();
                    }
                    if ui
                        .add_enabled(
                            running
                                && monitor_ready
                                && snapshot.seconds > 0.0
                                && !snapshot.recording
                                && !snapshot.playing,
                            egui::Button::new("Loop in headphones"),
                        )
                        .on_hover_text("Enables Mic Test and replays from the start. Use headphones to avoid feedback.")
                        .clicked()
                    {
                        self.start_tuning_replay();
                    }
                    if ui
                        .add_enabled(snapshot.seconds > 0.0, egui::Button::new("Clear clip"))
                        .clicked()
                    {
                        self.params.tuning.clear();
                    }
                });

                let mut comparison = snapshot.comparison;
                let mut match_loudness = snapshot.match_loudness;
                ui.horizontal(|ui| {
                    ui.label("Listen to:");
                    ui.selectable_value(&mut comparison, Comparison::Original, "Original");
                    ui.selectable_value(&mut comparison, Comparison::Processed, "Processed");
                });
                ui.checkbox(&mut match_loudness, "Match comparison loudness (approximate RMS)")
                    .on_hover_text("Attenuates the louder version using smoothed RMS. Never boosts the quieter version; not a limiter.");
                if comparison != snapshot.comparison || match_loudness != snapshot.match_loudness {
                    self.params.tuning.set_comparison(comparison, match_loudness);
                }
                if snapshot.recording {
                    ui.label(format!(
                        "Recording: {:.1} / {MAX_RECORD_SECONDS} s",
                        snapshot.seconds
                    ));
                    ui.add(egui::ProgressBar::new(
                        snapshot.seconds / MAX_RECORD_SECONDS as f32,
                    ));
                } else if snapshot.playing {
                    ui.label(format!(
                        "Looping: {:.1} / {:.1} s",
                        snapshot.position_seconds, snapshot.seconds
                    ));
                    ui.add(egui::ProgressBar::new(
                        snapshot.position_seconds / snapshot.seconds.max(0.01),
                    ));
                    if !self.params.monitor_enabled.load(Ordering::Relaxed) {
                        ui.label("Mic Test is off; enable it to hear the replay.");
                    }
                } else {
                    ui.label(format!("Recorded clip: {:.1} s", snapshot.seconds));
                }
                if !running {
                    ui.label("Start processing to record or replay.");
                } else if !monitor_ready {
                    ui.label("Choose a headphone monitor device and Apply to enable replay.");
                }
                ui.small("Level meters show the live cable signal, not replay. Mute silences both.");
            });
    }
}
