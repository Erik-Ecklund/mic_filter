#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod dsp;
mod ui;

use std::sync::Arc;

use parking_lot::Mutex;
use tracing_subscriber::EnvFilter;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::audio::{AudioEngine, LevelMeters};
use crate::config::{load_config, LiveParams};
use crate::ui::{make_tray_icon_rgba, AppState, MicProcessorApp};

fn main() -> eframe::Result<()> {
    init_logging();

    let config = load_config();
    let params = LiveParams::from_config(&config);
    let meters = LevelMeters::new();
    let engine = Arc::new(AudioEngine::new(params.clone(), meters.clone()));
    let quit_requested = Arc::new(Mutex::new(false));

    let tray_menu = Menu::new();
    let item_show = MenuItem::new("Open settings", true, None);
    let item_start = MenuItem::new("Start", true, None);
    let item_stop = MenuItem::new("Stop", true, None);
    let item_mute = MenuItem::new("Toggle mute", true, None);
    let item_quit = MenuItem::new("Quit", true, None);
    let _ = tray_menu.append_items(&[
        &item_show,
        &PredefinedMenuItem::separator(),
        &item_start,
        &item_stop,
        &item_mute,
        &PredefinedMenuItem::separator(),
        &item_quit,
    ]);

    let (rgba, w, h) = make_tray_icon_rgba();
    let icon = Icon::from_rgba(rgba, w, h).expect("tray icon");
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Mic Processor")
        .with_icon(icon)
        .build()
        .ok();

    let show_id = item_show.id().clone();
    let start_id = item_start.id().clone();
    let stop_id = item_stop.id().clone();
    let mute_id = item_mute.id().clone();
    let quit_id = item_quit.id().clone();

    let mut state = AppState::new(
        config.clone(),
        params.clone(),
        engine.clone(),
        meters,
        quit_requested.clone(),
    );

    if state.config.output_device.is_none() {
        if let Some(vb) = state.outputs.iter().find(|d| d.looks_like_vb_cable) {
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
    }

    if config.start_processing {
        state.start_engine();
    }

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([520.0, 720.0])
        .with_title("Mic Processor")
        .with_visible(!config.start_minimized);

    let mut app = MicProcessorApp { state };
    let menu_channel = MenuEvent::receiver();

    eframe::run_simple_native(
        "Mic Processor",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        move |ctx, frame| {
            while let Ok(event) = menu_channel.try_recv() {
                if event.id == show_id {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == start_id {
                    app.state.start_engine();
                } else if event.id == stop_id {
                    app.state.stop_engine();
                } else if event.id == mute_id {
                    let m = app
                        .state
                        .params
                        .muted
                        .load(std::sync::atomic::Ordering::Relaxed);
                    app.state
                        .params
                        .muted
                        .store(!m, std::sync::atomic::Ordering::Relaxed);
                } else if event.id == quit_id {
                    *app.state.quit_requested.lock() = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }

            if *app.state.quit_requested.lock() {
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
        if let Some(dir) = path.parent() {
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
}
