use crate::audio::db_to_meter;
use eframe::egui;
use std::sync::atomic::Ordering;

pub(super) fn slider_f32(
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
pub(super) fn colored_level_meter(
    ui: &mut egui::Ui,
    label: &str,
    level_db: f32,
    hold_db: f32,
    tip: &str,
) {
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
                paint_zone(&painter, rect, 0.0, g, egui::Color32::from_rgb(45, 200, 75));
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

fn paint_zone(painter: &egui::Painter, full: egui::Rect, t0: f32, t1: f32, color: egui::Color32) {
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
