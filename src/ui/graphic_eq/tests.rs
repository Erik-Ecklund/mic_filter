use super::*;

fn frame(
    ctx: &egui::Context,
    config: &mut GraphicEqConfig,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 500.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                graphic_eq_ui(ui, config);
            });
        },
    )
}

#[test]
fn flat_button_resets_all_faders_and_preamp_without_enabling_bypassed_filter() {
    let ctx = egui::Context::default();
    let mut config = GraphicEqConfig {
        enabled: false,
        preamp_db: -6.0,
        gains_db: [6.0; 10],
    };
    let before = config.clone();
    let output = frame(&ctx, &mut config, Vec::new());
    assert_eq!(config, before);
    let button = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Flat" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("Flat button text must be visible in the narrow editor");
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut config,
            vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    assert_eq!(config.gains_db, [0.0; 10]);
    assert_eq!(config.preamp_db, 0.0);
    assert!(!config.enabled);
}
