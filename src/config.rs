use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use atomic_float::AtomicF32;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "mic-processor";
const CONFIG_NAME: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    /// Local playback device for mic-test monitoring (headphones/speakers).
    pub monitor_device: Option<String>,
    pub highpass_enabled: bool,
    pub highpass_hz: f32,
    pub rnnoise_enabled: bool,
    pub expander: ExpanderConfig,
    pub compressor: CompressorConfig,
    pub master_gain_db: f32,
    pub monitor_enabled: bool,
    pub monitor_gain_db: f32,
    pub start_processing: bool,
    pub start_minimized: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
Self {
            input_device: None,
            output_device: None,
            monitor_device: None,
            highpass_enabled: true,
            highpass_hz: 80.0,
            rnnoise_enabled: true,
            expander: ExpanderConfig::default(),
            compressor: CompressorConfig::default(),
            master_gain_db: 0.0,
            monitor_enabled: false,
            monitor_gain_db: -6.0,
            start_processing: true,
            start_minimized: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExpanderConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub hold_ms: f32,
    pub range_db: f32,
}

impl Default for ExpanderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_db: -40.0,
            ratio: 3.0,
            attack_ms: 5.0,
            release_ms: 80.0,
            hold_ms: 50.0,
            range_db: 20.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 6.0,
            makeup_db: 6.0,
        }
    }
}

pub fn config_path() -> anyhow::Result<PathBuf> {
    let base = directories::ProjectDirs::from("com", "mic-processor", APP_DIR)
        .map(|d| d.config_dir().to_path_buf())
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_dir().join(APP_DIR)))
        .ok_or_else(|| anyhow::anyhow!("could not resolve config directory"))?;
    Ok(base.join(CONFIG_NAME))
}

pub fn load_config() -> AppConfig {
    let Ok(path) = config_path() else {
        return AppConfig::default();
    };
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
            tracing::warn!(%err, path = %path.display(), "invalid config; using defaults");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(cfg: &AppConfig) -> anyhow::Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(cfg)?;
    fs::write(path, text)?;
    Ok(())
}

/// Live parameters shared between UI and the realtime DSP thread.
pub struct LiveParams {
    pub highpass_enabled: AtomicBool,
    pub highpass_hz: AtomicF32,
    pub rnnoise_enabled: AtomicBool,
    pub expander_enabled: AtomicBool,
    pub expander_threshold_db: AtomicF32,
    pub expander_ratio: AtomicF32,
    pub expander_attack_ms: AtomicF32,
    pub expander_release_ms: AtomicF32,
    pub expander_hold_ms: AtomicF32,
    pub expander_range_db: AtomicF32,
    pub compressor_enabled: AtomicBool,
    pub compressor_threshold_db: AtomicF32,
    pub compressor_ratio: AtomicF32,
    pub compressor_attack_ms: AtomicF32,
    pub compressor_release_ms: AtomicF32,
    pub compressor_knee_db: AtomicF32,
    pub compressor_makeup_db: AtomicF32,
pub master_gain_db: AtomicF32,
    pub monitor_enabled: AtomicBool,
    pub monitor_gain_db: AtomicF32,
    pub muted: AtomicBool,
    pub running: AtomicBool,
}

impl LiveParams {
    pub fn from_config(cfg: &AppConfig) -> Arc<Self> {
        Arc::new(Self {
            highpass_enabled: AtomicBool::new(cfg.highpass_enabled),
            highpass_hz: AtomicF32::new(cfg.highpass_hz),
            rnnoise_enabled: AtomicBool::new(cfg.rnnoise_enabled),
            expander_enabled: AtomicBool::new(cfg.expander.enabled),
            expander_threshold_db: AtomicF32::new(cfg.expander.threshold_db),
            expander_ratio: AtomicF32::new(cfg.expander.ratio),
            expander_attack_ms: AtomicF32::new(cfg.expander.attack_ms),
            expander_release_ms: AtomicF32::new(cfg.expander.release_ms),
            expander_hold_ms: AtomicF32::new(cfg.expander.hold_ms),
            expander_range_db: AtomicF32::new(cfg.expander.range_db),
            compressor_enabled: AtomicBool::new(cfg.compressor.enabled),
            compressor_threshold_db: AtomicF32::new(cfg.compressor.threshold_db),
            compressor_ratio: AtomicF32::new(cfg.compressor.ratio),
            compressor_attack_ms: AtomicF32::new(cfg.compressor.attack_ms),
            compressor_release_ms: AtomicF32::new(cfg.compressor.release_ms),
            compressor_knee_db: AtomicF32::new(cfg.compressor.knee_db),
            compressor_makeup_db: AtomicF32::new(cfg.compressor.makeup_db),
master_gain_db: AtomicF32::new(cfg.master_gain_db),
            monitor_enabled: AtomicBool::new(cfg.monitor_enabled),
            monitor_gain_db: AtomicF32::new(cfg.monitor_gain_db),
            muted: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

pub fn apply_config(&self, cfg: &AppConfig) {
        self.highpass_enabled
            .store(cfg.highpass_enabled, Ordering::Relaxed);
        self.highpass_hz.store(cfg.highpass_hz, Ordering::Relaxed);
        self.rnnoise_enabled
            .store(cfg.rnnoise_enabled, Ordering::Relaxed);
        self.expander_enabled
            .store(cfg.expander.enabled, Ordering::Relaxed);
        self.expander_threshold_db
            .store(cfg.expander.threshold_db, Ordering::Relaxed);
        self.expander_ratio
            .store(cfg.expander.ratio, Ordering::Relaxed);
        self.expander_attack_ms
            .store(cfg.expander.attack_ms, Ordering::Relaxed);
        self.expander_release_ms
            .store(cfg.expander.release_ms, Ordering::Relaxed);
        self.expander_hold_ms
            .store(cfg.expander.hold_ms, Ordering::Relaxed);
        self.expander_range_db
            .store(cfg.expander.range_db, Ordering::Relaxed);
        self.compressor_enabled
            .store(cfg.compressor.enabled, Ordering::Relaxed);
        self.compressor_threshold_db
            .store(cfg.compressor.threshold_db, Ordering::Relaxed);
        self.compressor_ratio
            .store(cfg.compressor.ratio, Ordering::Relaxed);
        self.compressor_attack_ms
            .store(cfg.compressor.attack_ms, Ordering::Relaxed);
        self.compressor_release_ms
            .store(cfg.compressor.release_ms, Ordering::Relaxed);
        self.compressor_knee_db
            .store(cfg.compressor.knee_db, Ordering::Relaxed);
        self.compressor_makeup_db
            .store(cfg.compressor.makeup_db, Ordering::Relaxed);
self.master_gain_db
            .store(cfg.master_gain_db, Ordering::Relaxed);
        self.monitor_enabled
            .store(cfg.monitor_enabled, Ordering::Relaxed);
        self.monitor_gain_db
            .store(cfg.monitor_gain_db, Ordering::Relaxed);
    }

    pub fn snapshot_into(&self, cfg: &mut AppConfig) {
        cfg.highpass_enabled = self.highpass_enabled.load(Ordering::Relaxed);
        cfg.highpass_hz = self.highpass_hz.load(Ordering::Relaxed);
        cfg.rnnoise_enabled = self.rnnoise_enabled.load(Ordering::Relaxed);
        cfg.expander.enabled = self.expander_enabled.load(Ordering::Relaxed);
        cfg.expander.threshold_db = self.expander_threshold_db.load(Ordering::Relaxed);
        cfg.expander.ratio = self.expander_ratio.load(Ordering::Relaxed);
        cfg.expander.attack_ms = self.expander_attack_ms.load(Ordering::Relaxed);
        cfg.expander.release_ms = self.expander_release_ms.load(Ordering::Relaxed);
        cfg.expander.hold_ms = self.expander_hold_ms.load(Ordering::Relaxed);
        cfg.expander.range_db = self.expander_range_db.load(Ordering::Relaxed);
        cfg.compressor.enabled = self.compressor_enabled.load(Ordering::Relaxed);
        cfg.compressor.threshold_db = self.compressor_threshold_db.load(Ordering::Relaxed);
        cfg.compressor.ratio = self.compressor_ratio.load(Ordering::Relaxed);
        cfg.compressor.attack_ms = self.compressor_attack_ms.load(Ordering::Relaxed);
        cfg.compressor.release_ms = self.compressor_release_ms.load(Ordering::Relaxed);
        cfg.compressor.knee_db = self.compressor_knee_db.load(Ordering::Relaxed);
cfg.compressor.makeup_db = self.compressor_makeup_db.load(Ordering::Relaxed);
        cfg.master_gain_db = self.master_gain_db.load(Ordering::Relaxed);
        cfg.monitor_enabled = self.monitor_enabled.load(Ordering::Relaxed);
        cfg.monitor_gain_db = self.monitor_gain_db.load(Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn default_config_roundtrips_through_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let back: AppConfig = toml::from_str(&text).expect("deserialize");
        assert_eq!(back.highpass_hz, cfg.highpass_hz);
        assert_eq!(back.rnnoise_enabled, cfg.rnnoise_enabled);
        assert_eq!(back.expander.threshold_db, cfg.expander.threshold_db);
        assert_eq!(back.compressor.ratio, cfg.compressor.ratio);
        assert_eq!(back.master_gain_db, cfg.master_gain_db);
    }

    #[test]
    fn live_params_mirror_config_and_snapshot() {
        let mut cfg = AppConfig::default();
        cfg.expander.threshold_db = -33.0;
        cfg.compressor.makeup_db = 9.0;
        cfg.master_gain_db = 3.5;
        cfg.rnnoise_enabled = false;

        let live = LiveParams::from_config(&cfg);
        assert!(!live.rnnoise_enabled.load(Ordering::Relaxed));
        assert!((live.expander_threshold_db.load(Ordering::Relaxed) - (-33.0)).abs() < 1e-6);
        assert!((live.compressor_makeup_db.load(Ordering::Relaxed) - 9.0).abs() < 1e-6);

        live.master_gain_db.store(-1.25, Ordering::Relaxed);
        live.highpass_enabled.store(false, Ordering::Relaxed);

        let mut out = AppConfig::default();
        live.snapshot_into(&mut out);
        assert!((out.master_gain_db - (-1.25)).abs() < 1e-6);
        assert!(!out.highpass_enabled);
        assert!((out.expander.threshold_db - (-33.0)).abs() < 1e-6);
    }

    #[test]
    fn apply_config_updates_atomics() {
        let live = LiveParams::from_config(&AppConfig::default());
        let mut cfg = AppConfig::default();
        cfg.highpass_hz = 120.0;
        cfg.expander.enabled = false;
        cfg.compressor.knee_db = 12.0;
        live.apply_config(&cfg);
        assert!((live.highpass_hz.load(Ordering::Relaxed) - 120.0).abs() < 1e-6);
        assert!(!live.expander_enabled.load(Ordering::Relaxed));
        assert!((live.compressor_knee_db.load(Ordering::Relaxed) - 12.0).abs() < 1e-6);
    }
}
