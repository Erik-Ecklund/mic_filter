#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod dsp;
mod ui;

use std::rc::Rc;

use tracing_subscriber::EnvFilter;

use crate::audio::{AudioEngine, LevelMeters};
use crate::config::{LiveParams, load_config};
use crate::ui::{AppState, MicProcessorApp, TrayController};

fn main() -> eframe::Result<()> {
    init_logging();

    let config = load_config();
    let params = LiveParams::from_config(&config);
    let meters = LevelMeters::new();
    let engine = Rc::new(AudioEngine::new(params.clone(), meters.clone()));
    let mut tray = TrayController::new();

    let mut state = AppState::new(config.clone(), params.clone(), engine.clone(), meters);

    if let (None, Some(vb)) = (
        &state.config.output_device,
        state.outputs.iter().find(|d| d.looks_like_vb_cable),
    ) {
        let pick = state
            .outputs
            .iter()
            .find(|d| {
                let l = d.name.to_ascii_lowercase();
                l.contains("cable input") || (d.looks_like_vb_cable && l.contains("input"))
            })
            .unwrap_or(vb);
        state.config.output_device = Some(pick.name.clone());
    }

    if config.start_processing {
        state.start_engine();
    }

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([520.0, 720.0])
        .with_title("Mic Processor")
        .with_visible(!config.start_minimized);

    let mut app = MicProcessorApp { state };

    eframe::run_simple_native(
        "Mic Processor",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        move |ctx, frame| {
            tray.process_events(&mut app.state, ctx);
            if app.state.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            eframe::App::update(&mut app, ctx, frame);
        },
    )
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();

    if let Ok(path) = config::config_path() {
        let Some(dir) = path.parent() else {
            return;
        };
        let log_path = dir.join("mic-processor.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(file, "---- mic_filter start ----");
            tracing::info!(path = %log_path.display(), "config/log directory ready");
        }
    }
}
