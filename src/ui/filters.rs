use eframe::egui;

use crate::config::{FilterConfig, FilterKind, FilterSettings};

enum ChainEdit {
    Move(usize, usize),
    Remove(usize),
}

pub(super) fn filter_chain_ui(
    ui: &mut egui::Ui,
    filters: &mut Vec<FilterConfig>,
    next_id: &mut u64,
) -> bool {
    ui.heading("Filter chain");
    ui.label("Processed top to bottom. Changes apply live; Save persists them.");
    let before = filters.clone();
    let count = filters.len();
    let mut edit = None;
    for (index, filter) in filters.iter_mut().enumerate() {
        // Stable IDs keep both widget state and processor state attached to
        // an instance rather than its current position.
        ui.push_id(filter.id, |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("{}. {}", index + 1, filter.settings.kind().label()));
                    ui.checkbox(filter.settings.enabled_mut(), "Enabled")
                        .on_hover_text("Uncheck to bypass without removing this filter.");
                    if ui.add_enabled(index > 0, egui::Button::new("Up")).clicked() {
                        edit = Some(ChainEdit::Move(index, index - 1));
                    }
                    if ui
                        .add_enabled(index + 1 < count, egui::Button::new("Down"))
                        .clicked()
                    {
                        edit = Some(ChainEdit::Move(index, index + 1));
                    }
                    if ui.button("Remove").clicked() {
                        edit = Some(ChainEdit::Remove(index));
                    }
                });
                egui::CollapsingHeader::new("Settings")
                    .default_open(true)
                    .show(ui, |ui| filter_settings_ui(ui, &mut filter.settings));
            });
        });
        ui.add_space(4.0);
    }
    // Apply structural edits after drawing, so iteration and widget IDs stay valid.
    match edit {
        Some(ChainEdit::Move(a, b)) => filters.swap(a, b),
        Some(ChainEdit::Remove(index)) => {
            filters.remove(index);
        }
        None => {}
    }
    if filters.is_empty() {
        ui.weak("No filters: unprocessed audio passes through (mute still applies).");
    }
    ui.menu_button("Add filter", |ui| {
        for kind in FilterKind::ALL {
            if ui.button(kind.label()).clicked() {
                // Do not normally recycle removed IDs; a remove/add between
                // audio blocks must still create a genuinely new processor.
                while *next_id == 0 || filters.iter().any(|f| f.id == *next_id) {
                    *next_id = next_id.wrapping_add(1);
                }
                filters.push(FilterConfig::new(*next_id, kind.default_settings()));
                *next_id = next_id.wrapping_add(1);
                ui.close_menu();
            }
        }
    });
    *filters != before
}

fn filter_settings_ui(ui: &mut egui::Ui, settings: &mut FilterSettings) {
    match settings {
        FilterSettings::HighPass(c) => {
            ui.weak("Remove low-frequency rumble. Higher cutoffs thin the low end.");
            slider(
                ui,
                &mut c.cutoff_hz,
                40.0..=200.0,
                "Cutoff Hz",
                "Usually around 80 Hz. This filter runs at its position in the chain.",
            );
        }
        FilterSettings::Rnnoise(_) => {
            ui.weak("Neural noise suppression at 48 kHz, in 10 ms frames.");
            ui.small("Each RNNoise instance has independent state. Prefer one near the start.");
        }
        FilterSettings::Expander(c) => {
            ui.weak("Downward expander / soft gate: quiet residual noise in pauses.");
            slider(
                ui,
                &mut c.threshold_db,
                -80.0..=0.0,
                "Threshold dB",
                "Expansion starts below this level. Lower it if quiet speech is cut.",
            );
            slider(
                ui,
                &mut c.ratio,
                1.0..=10.0,
                "Ratio",
                "1 = no expansion; higher = stronger attenuation.",
            );
            slider(
                ui,
                &mut c.attack_ms,
                0.1..=50.0,
                "Attack ms",
                "Gain reduction response time; very fast settings can sound choppy.",
            );
            slider(
                ui,
                &mut c.release_ms,
                10.0..=500.0,
                "Release ms",
                "Gain recovery time; very fast settings can cause pumping.",
            );
            slider(
                ui,
                &mut c.hold_ms,
                0.0..=300.0,
                "Hold ms",
                "Briefly hold the gate open near threshold after speech.",
            );
            slider(
                ui,
                &mut c.range_db,
                0.0..=60.0,
                "Range dB",
                "Maximum attenuation in silence.",
            );
        }
        FilterSettings::Compressor(c) => {
            ui.weak("Soft-knee compressor: even out loud and quiet speech.");
            slider(
                ui,
                &mut c.threshold_db,
                -60.0..=0.0,
                "Threshold dB",
                "Compression engages above this level.",
            );
            slider(
                ui,
                &mut c.ratio,
                1.0..=20.0,
                "Ratio",
                "Compression strength; higher ratios approach limiting.",
            );
            slider(
                ui,
                &mut c.attack_ms,
                0.1..=100.0,
                "Attack ms",
                "Fast = tighter peak control; slow = more transient punch.",
            );
            slider(
                ui,
                &mut c.release_ms,
                10.0..=500.0,
                "Release ms",
                "Gain recovery time after a loud phrase.",
            );
            slider(
                ui,
                &mut c.knee_db,
                0.0..=24.0,
                "Knee dB",
                "Wider knees produce a smoother transition into compression.",
            );
            slider(
                ui,
                &mut c.makeup_db,
                -12.0..=24.0,
                "Makeup dB",
                "Gain added after this compressor to restore loudness.",
            );
        }
        FilterSettings::Gain(c) => {
            ui.weak("Gain trim at this position in the chain.");
            slider(
                ui,
                &mut c.gain_db,
                -24.0..=24.0,
                "Gain dB",
                "Non-unity gain clips to digital full scale. Monitor volume is independent.",
            );
        }
    }
}

fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
    tip: &str,
) {
    ui.add(egui::Slider::new(value, range).text(label))
        .on_hover_text(tip);
}
