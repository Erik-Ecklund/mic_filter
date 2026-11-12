use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::*;
use crate::audio::{AudioEngine, LevelMeters};
use crate::config::{AppConfig, LiveParams};

pub(super) fn app(config: AppConfig) -> MicProcessorApp {
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
            profile_ui: super::profiles::ProfileUi::default(),
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

#[test]
fn profile_load_publishes_limiter_and_filters_without_touching_global_state() {
    let mut config = AppConfig {
        input_device: Some("Mic".into()),
        monitor_gain_db: -15.0,
        ..AppConfig::default()
    };
    config.limiter.enabled = true;
    config.limiter.ceiling_db = -3.0;
    config.save_new_profile("Meetings").unwrap();
    config.filters.clear();
    config.limiter.enabled = false;
    let mut app = app(config);
    app.state.params.tuning.record();
    app.state.params.muted.store(true, Ordering::Relaxed);
    app.state.load_named_profile("Meetings").unwrap();
    let live = app.state.params.chain.lock().clone();
    assert_eq!(live.filters, app.state.config.filters);
    assert_eq!(live.limiter, app.state.config.limiter);
    assert!(live.limiter.enabled);
    assert_eq!(app.state.config.input_device.as_deref(), Some("Mic"));
    assert_eq!(
        app.state.params.monitor_gain_db.load(Ordering::Relaxed),
        -15.0
    );
    assert!(app.state.params.muted.load(Ordering::Relaxed));
    assert!(app.state.params.tuning.snapshot().recording);
    let before = app.state.config.clone();
    assert!(app.state.load_named_profile("Missing").is_err());
    assert_eq!(app.state.config, before);
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.show(ctx));
    assert_eq!(app.state.config, before);
}
