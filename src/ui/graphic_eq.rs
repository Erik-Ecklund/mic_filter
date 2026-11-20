use eframe::egui;

use crate::config::{GRAPHIC_EQ_FREQUENCIES, GraphicEqConfig};

#[cfg(test)]
mod tests;

pub(super) fn graphic_eq_ui(ui: &mut egui::Ui, config: &mut GraphicEqConfig) {
    ui.weak("Ten fixed-frequency bands. Raise a fader to boost, lower it to cut.");
    ui.small(
        "Bands overlap. Reduce preamp for headroom when boosting; the final limiter is separate.",
    );
    ui.add(egui::Slider::new(&mut config.preamp_db, -24.0..=12.0).text("Preamp dB"))
        .on_hover_text(
            "Applied before the EQ without hard clipping. Negative trim leaves headroom.",
        );
    if ui
        .button("Flat")
        .on_hover_text("Reset all bands and preamp to 0 dB; keep bypass unchanged.")
        .clicked()
    {
        config.reset_flat();
    }
    ui.small("Band centers in Hz / kHz. Scroll horizontally to reach all bands.");
    egui::ScrollArea::horizontal()
        .id_salt("graphic_eq_faders")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for ((index, gain), frequency) in config
                    .gains_db
                    .iter_mut()
                    .enumerate()
                    .zip(GRAPHIC_EQ_FREQUENCIES)
                {
                    ui.push_id(index, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(62.0, 200.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.set_width(62.0);
                                let label = if frequency >= 1000.0 {
                                    format!("{}k", frequency / 1000.0)
                                } else {
                                    format!("{frequency}")
                                };
                                ui.label(label).on_hover_text(format!("{frequency} Hz"));
                                ui.scope(|ui| {
                                    ui.spacing_mut().slider_width = 132.0;
                                    ui.add(
                                        egui::Slider::new(gain, -12.0..=12.0)
                                            .vertical()
                                            .show_value(false),
                                    )
                                    .on_hover_text(format!("{frequency} Hz gain in dB"));
                                });
                                ui.add(
                                    egui::DragValue::new(gain)
                                        .range(-12.0..=12.0)
                                        .speed(0.1)
                                        .fixed_decimals(1)
                                        .suffix(" dB"),
                                )
                                .on_hover_text("Drag or type a gain between −12 and +12 dB.");
                            },
                        );
                    });
                }
            });
        });
}
