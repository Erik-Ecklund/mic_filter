use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::*;
use crate::audio::{AudioEngine, LevelMeters};
use crate::config::{AppConfig, LiveParams};

fn app(config: AppConfig) -> MicProcessorApp {
    let params = LiveParams::from_config(&config);
    let meters = LevelMeters::new();
    MicProcessorApp {
        state: AppState {
            config,
            params: params.clone(),
            engine: Rc::new(AudioEngine::new(params, meters.clone())),
            meters,
            inputs: Vec::new(),
            outputs: Vec::new(),
            status_message: String::new(),
            quit_requested: false,
            next_filter_id: 10,
        },
    }
}

#[test]
fn panels_render_without_audio_devices_or_config_mutation() {
    for mut config in [
        AppConfig::default(),
        AppConfig {
            filters: Vec::new(),
            ..AppConfig::default()
        },
    ] {
        config.start_processing = false;
        let mut app = app(config.clone());
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.show(ctx));
        }
        assert_eq!(app.state.config, config);
        assert!(!app.state.engine.is_running());
    }
}

#[test]
fn shared_actions_keep_monitor_state_in_sync() {
    let mut app = app(AppConfig::default());
    app.state.toggle_monitor();
    assert!(app.state.config.monitor_enabled);
    assert!(app.state.params.monitor_enabled.load(Ordering::Relaxed));
    app.state.set_monitor_enabled(false);
    assert!(!app.state.config.monitor_enabled);
    assert!(!app.state.params.monitor_enabled.load(Ordering::Relaxed));
    app.state.toggle_mute();
    assert!(app.state.params.muted.load(Ordering::Relaxed));
    app.state.toggle_mute();
    assert!(!app.state.params.muted.load(Ordering::Relaxed));
}

#[test]
fn tuning_replay_requires_running_monitor_and_does_not_enable_it_on_failure() {
    let mut app = app(AppConfig::default());
    app.state.set_monitor_enabled(false);
    app.state.start_tuning_replay();
    assert!(!app.state.params.tuning.snapshot().playing);
    assert!(!app.state.params.monitor_enabled.load(Ordering::Relaxed));
    assert!(app.state.status_message.contains("headphone"));
}

#[test]
fn tuning_panel_renders_recording_state_without_persisting_clip_settings() {
    let mut app = app(AppConfig::default());
    let before = app.state.config.clone();
    app.state.params.tuning.record();
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.show(ctx));
    assert_eq!(app.state.config, before);
    app.state.engine.stop();
    assert!(!app.state.params.tuning.snapshot().recording);
}
