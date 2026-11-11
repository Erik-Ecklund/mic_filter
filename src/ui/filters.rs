use eframe::egui;

use crate::config::{FilterConfig, FilterKind, FilterSettings};

enum ChainEdit {
    Move(usize, usize),
    Drop { filter_id: u64, slot: usize },
    Remove(usize),
}

#[derive(Clone, Copy)]
struct FilterDrag {
    chain_id: egui::Id,
    filter_id: u64,
}

pub(super) fn filter_chain_ui(
    ui: &mut egui::Ui,
    filters: &mut Vec<FilterConfig>,
    next_id: &mut u64,
) -> bool {
    ui.heading("Filter chain");
    ui.label("Processed top to bottom. Drag a grip to reorder, or use Up / Down.");
    ui.small("Changes apply live on drop. Save persists the chain.");
    let before = filters.clone();
    let chain_id = ui.make_persistent_id("filter_chain");
    let count = filters.len();
    let mut edit = None;
    let mut cards = Vec::with_capacity(count);
    for (index, filter) in filters.iter_mut().enumerate() {
        let payload = FilterDrag {
            chain_id,
            filter_id: filter.id,
        };
        let dragging = egui::DragAndDrop::payload::<FilterDrag>(ui.ctx())
            .is_some_and(|drag| drag.chain_id == chain_id && drag.filter_id == filter.id);
        let mut frame = egui::Frame::group(ui.style());
        if dragging {
            frame = frame
                .fill(ui.visuals().selection.bg_fill.linear_multiply(0.25))
                .stroke(ui.visuals().selection.stroke);
        }
        // Stable IDs keep both widget state and processor state attached to
        // an instance rather than its current position.
        let card = ui.push_id(filter.id, |ui| {
            frame
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        drag_handle(ui, payload);
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
                })
                .response
                .rect
        });
        cards.push(card.inner);
        ui.add_space(4.0);
    }

    if let Some(drop) = drag_edit(ui, filters, &cards, chain_id) {
        edit = Some(drop);
    }

    // Apply structural edits after drawing, so iteration and widget IDs stay valid.
    match edit {
        Some(ChainEdit::Move(a, b)) => filters.swap(a, b),
        Some(ChainEdit::Drop { filter_id, slot }) => {
            move_filter_to_slot(filters, filter_id, slot);
        }
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
fn drag_edit(
    ui: &egui::Ui,
    filters: &[FilterConfig],
    cards: &[egui::Rect],
    chain_id: egui::Id,
) -> Option<ChainEdit> {
    let drag = egui::DragAndDrop::payload::<FilterDrag>(ui.ctx())?;
    if drag.chain_id != chain_id || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        return None;
    }
    let source = filters.iter().find(|filter| filter.id == drag.filter_id)?;
    let pointer = ui.ctx().pointer_interact_pos()?;
    egui::Area::new(chain_id.with("drag_preview"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(pointer + egui::vec2(14.0, 14.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(format!("Move {}", source.settings.kind().label()));
            });
        });
    ui.ctx().request_repaint();
    let slot = drop_slot(cards, pointer, ui.clip_rect())?;
    paint_insertion(ui, cards, slot);
    if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary)) {
        egui::DragAndDrop::clear_payload(ui.ctx());
        Some(ChainEdit::Drop {
            filter_id: drag.filter_id,
            slot,
        })
    } else {
        if ui.input(|input| input.pointer.primary_down()) {
            auto_scroll(ui, pointer);
        }
        None
    }
}

fn drag_handle(ui: &mut egui::Ui, payload: FilterDrag) {
    let (_, rect) = ui.allocate_space(egui::vec2(18.0, ui.spacing().interact_size.y.max(22.0)));
    let response = ui
        .interact(
            rect,
            payload.chain_id.with(("drag_handle", payload.filter_id)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Grab);
    if response.drag_started_by(egui::PointerButton::Primary) {
        response.dnd_set_drag_payload(payload);
    }
    let color = if response.dragged() {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    for x in [-3.0, 3.0] {
        for y in [-5.0, 0.0, 5.0] {
            ui.painter()
                .circle_filled(rect.center() + egui::vec2(x, y), 1.5, color);
        }
    }
    response.on_hover_text("Drag to reorder. Drop above or below a filter; Escape cancels.");
}

/// Slots are between cards, including before the first and after the last.
fn drop_slot(cards: &[egui::Rect], pointer: egui::Pos2, clip: egui::Rect) -> Option<usize> {
    let bounds = cards
        .iter()
        .copied()
        .reduce(|a, b| a.union(b))?
        .expand2(egui::vec2(0.0, 4.0));
    if !clip.contains(pointer) || !bounds.contains(pointer) {
        return None;
    }
    Some(
        cards
            .iter()
            .position(|card| pointer.y < card.center().y)
            .unwrap_or(cards.len()),
    )
}

fn paint_insertion(ui: &egui::Ui, cards: &[egui::Rect], slot: usize) {
    let rect = if slot == cards.len() {
        cards[slot - 1]
    } else {
        cards[slot]
    };
    let y = if slot == cards.len() {
        rect.bottom() + 2.0
    } else {
        rect.top() - 2.0
    };
    let left = egui::pos2(rect.left(), y);
    let right = egui::pos2(rect.right(), y);
    let color = ui.visuals().selection.stroke.color;
    ui.painter()
        .line_segment([left, right], egui::Stroke::new(3.0_f32, color));
    ui.painter().circle_filled(left, 3.0, color);
}

fn auto_scroll(ui: &egui::Ui, pointer: egui::Pos2) {
    let clip = ui.clip_rect();
    let edge = 40.0;
    let direction = if pointer.y < clip.top() + edge {
        (clip.top() + edge - pointer.y) / edge
    } else if pointer.y > clip.bottom() - edge {
        -(pointer.y - (clip.bottom() - edge)) / edge
    } else {
        0.0
    };
    let dt = ui.input(|input| input.stable_dt.min(0.05));
    ui.scroll_with_delta(egui::vec2(0.0, direction * 360.0 * dt));
}

/// Move, rather than swap, so every other filter retains its relative order.
fn move_filter_to_slot(filters: &mut Vec<FilterConfig>, filter_id: u64, slot: usize) -> bool {
    if slot > filters.len() {
        return false;
    }
    let Some(from) = filters.iter().position(|filter| filter.id == filter_id) else {
        return false;
    };
    let to = if slot > from { slot - 1 } else { slot };
    if from == to {
        return false;
    }
    let filter = filters.remove(from);
    filters.insert(to, filter);
    true
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

#[cfg(test)]
mod tests;
