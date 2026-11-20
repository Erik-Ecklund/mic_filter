use super::*;
use crate::config::GainConfig;

#[test]
fn every_filter_editor_renders_without_changing_settings() {
    let ctx = egui::Context::default();
    let mut filters: Vec<_> = FilterKind::ALL
        .into_iter()
        .enumerate()
        .map(|(i, kind)| FilterConfig::new(i as u64 + 1, kind.default_settings()))
        .collect();
    let before = filters.clone();
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    assert!(!filter_chain_ui(ui, &mut filters, &mut 100));
                });
            });
        });
    }
    assert_eq!(filters, before);
}

fn filters(count: u64) -> Vec<FilterConfig> {
    (1..=count)
        .map(|id| {
            FilterConfig::new(
                id,
                FilterSettings::Gain(GainConfig {
                    enabled: id != 3,
                    gain_db: id as f32,
                }),
            )
        })
        .collect()
}

fn ids(filters: &[FilterConfig]) -> Vec<u64> {
    filters.iter().map(|filter| filter.id).collect()
}

#[test]
fn insertion_moves_preserve_settings_and_other_filters_order() {
    let original = filters(4);
    let mut filters = original.clone();
    assert!(move_filter_to_slot(&mut filters, 1, 4));
    assert_eq!(
        filters,
        [
            original[1].clone(),
            original[2].clone(),
            original[3].clone(),
            original[0].clone()
        ]
    );
    assert!(move_filter_to_slot(&mut filters, 1, 0));
    assert_eq!(filters, original);
    assert!(move_filter_to_slot(&mut filters, 4, 1));
    assert_eq!(ids(&filters), [1, 4, 2, 3]);
}

#[test]
fn unchanged_or_invalid_drops_do_not_modify_the_chain() {
    let mut filters = filters(4);
    let original = filters.clone();
    for (id, slot) in [(2, 1), (2, 2), (99, 0), (1, 5)] {
        assert!(!move_filter_to_slot(&mut filters, id, slot));
        assert_eq!(filters, original);
    }
    assert!(!move_filter_to_slot(&mut Vec::new(), 1, 0));
}

#[test]
fn drop_slots_respect_card_midpoints_gaps_and_clipping() {
    let card =
        |top, bottom| egui::Rect::from_min_max(egui::pos2(10.0, top), egui::pos2(200.0, bottom));
    let cards = [card(10.0, 90.0), card(94.0, 124.0), card(128.0, 228.0)];
    let clip = card(0.0, 300.0);
    for (y, expected) in [
        (8.0, 0),
        (49.0, 0),
        (50.0, 1),
        (92.0, 1),
        (110.0, 2),
        (220.0, 3),
        (230.0, 3),
    ] {
        assert_eq!(drop_slot(&cards, egui::pos2(50.0, y), clip), Some(expected));
    }
    assert_eq!(drop_slot(&cards, egui::pos2(0.0, 50.0), clip), None);
    assert_eq!(drop_slot(&cards, egui::pos2(50.0, 240.0), clip), None);
    assert_eq!(
        drop_slot(&cards, egui::pos2(50.0, 50.0), card(100.0, 200.0)),
        None
    );
    assert_eq!(drop_slot(&[], egui::pos2(50.0, 50.0), clip), None);
}

struct TestUi {
    ctx: egui::Context,
    filters: Vec<FilterConfig>,
    chain_id: egui::Id,
    scrolling: bool,
}

impl TestUi {
    fn new(count: u64) -> Self {
        let mut ui = Self {
            ctx: egui::Context::default(),
            filters: filters(count),
            chain_id: egui::Id::NULL,
            scrolling: false,
        };
        ui.frame(Vec::new());
        ui
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> bool {
        let mut changed = false;
        let height = if self.scrolling { 260.0 } else { 1000.0 };
        let _ = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, height),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut show = |ui: &mut egui::Ui| {
                        self.chain_id = ui.make_persistent_id("filter_chain");
                        changed = filter_chain_ui(ui, &mut self.filters, &mut 100);
                    };
                    if self.scrolling {
                        egui::ScrollArea::vertical().show(ui, show);
                    } else {
                        show(ui);
                    }
                });
            },
        );
        changed
    }

    fn handle(&self, id: u64) -> egui::Pos2 {
        self.ctx
            .read_response(self.chain_id.with(("drag_handle", id)))
            .unwrap()
            .rect
            .center()
    }

    fn start_drag(&mut self, id: u64) {
        let start = self.handle(id);
        assert!(!self.frame(vec![egui::Event::PointerMoved(start)]));
        assert!(!self.frame(vec![button(start, true)]));
        assert!(!self.frame(vec![egui::Event::PointerMoved(
            start + egui::vec2(3.0, 0.0)
        )]));
        assert!(egui::DragAndDrop::has_payload_of_type::<FilterDrag>(
            &self.ctx
        ));
    }

    fn release_at(&mut self, target: egui::Pos2) -> bool {
        assert!(!self.frame(vec![egui::Event::PointerMoved(target)]));
        self.frame(vec![button(target, false)])
    }
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

#[test]
fn pointer_drag_moves_up_only_on_release_and_retains_settings() {
    let mut ui = TestUi::new(4);
    let original = ui.filters.clone();
    ui.start_drag(4);
    assert_eq!(ui.filters, original);
    let target = ui.handle(1);
    assert!(ui.release_at(target));
    assert_eq!(
        ui.filters,
        [
            original[3].clone(),
            original[0].clone(),
            original[1].clone(),
            original[2].clone()
        ]
    );
    assert!(!egui::DragAndDrop::has_payload_of_type::<FilterDrag>(
        &ui.ctx
    ));
}

#[test]
fn pointer_drag_moves_down_without_swapping_intervening_filters() {
    let mut ui = TestUi::new(4);
    ui.start_drag(1);
    let target = ui.handle(4);
    assert!(ui.release_at(target));
    assert_eq!(ids(&ui.filters), [2, 3, 1, 4]);
}

#[test]
fn pointer_can_drop_after_the_last_card() {
    let mut ui = TestUi::new(4);
    let step = ui.handle(2).y - ui.handle(1).y;
    ui.start_drag(1);
    let target = ui.handle(4) + egui::vec2(0.0, step * 0.65);
    assert!(ui.release_at(target));
    assert_eq!(ids(&ui.filters), [2, 3, 4, 1]);
}

#[test]
fn escape_and_drops_outside_the_list_cancel_reordering() {
    let mut ui = TestUi::new(4);
    let original = ui.filters.clone();
    ui.start_drag(4);
    let target = ui.handle(1);
    assert!(!ui.frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    }]));
    assert!(!ui.release_at(target));
    assert_eq!(ui.filters, original);
    ui.start_drag(1);
    assert!(!ui.release_at(egui::pos2(800.0, 400.0)));
    assert_eq!(ui.filters, original);
}

#[test]
fn dragging_a_card_title_does_not_start_filter_reordering() {
    let mut ui = TestUi::new(4);
    let start = ui.handle(1) + egui::vec2(32.0, 0.0);
    ui.frame(vec![egui::Event::PointerMoved(start)]);
    ui.frame(vec![button(start, true)]);
    let target = ui.handle(4);
    ui.frame(vec![egui::Event::PointerMoved(target)]);
    assert!(!egui::DragAndDrop::has_payload_of_type::<FilterDrag>(
        &ui.ctx
    ));
    assert!(!ui.frame(vec![button(target, false)]));
    assert_eq!(ids(&ui.filters), [1, 2, 3, 4]);
}

#[test]
fn dragging_near_the_viewport_edge_scrolls_the_filter_list() {
    let mut ui = TestUi::new(10);
    ui.scrolling = true;
    ui.frame(Vec::new());
    ui.frame(Vec::new());
    let before = ui.handle(1).y;
    ui.start_drag(1);
    let target = egui::pos2(ui.handle(1).x, 245.0);
    for _ in 0..40 {
        assert!(!ui.frame(vec![egui::Event::PointerMoved(target)]));
    }
    assert!(ui.handle(1).y < before - 10.0);
    ui.frame(vec![button(egui::pos2(800.0, 400.0), false)]);
    assert_eq!(ids(&ui.filters), (1..=10).collect::<Vec<_>>());
}

#[test]
fn dropping_in_the_same_position_does_not_report_a_change() {
    let mut ui = TestUi::new(1);
    ui.start_drag(1);
    assert!(!ui.release_at(ui.handle(1)));
    assert_eq!(ids(&ui.filters), [1]);
}

#[test]
fn payloads_from_another_chain_are_not_accepted() {
    let mut ui = TestUi::new(4);
    let original = ui.filters.clone();
    egui::DragAndDrop::set_payload(
        &ui.ctx,
        FilterDrag {
            chain_id: egui::Id::new("another_chain"),
            filter_id: 4,
        },
    );
    let target = ui.handle(1);
    assert!(!ui.frame(vec![
        egui::Event::PointerMoved(target),
        button(target, false)
    ]));
    assert_eq!(ui.filters, original);
}
