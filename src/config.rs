use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atomic_float::AtomicF32;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "mic-processor";
const CONFIG_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredConfig")]
pub struct AppConfig {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub monitor_device: Option<String>,
    pub filters: Vec<FilterConfig>,
    pub monitor_enabled: bool,
    pub monitor_gain_db: f32,
    pub start_processing: bool,
    pub start_minimized: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        StoredConfig::default().into()
    }
}

/// Read both the original singleton settings and the new ordered list.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct StoredConfig {
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
    pub filters: Option<Vec<FilterConfig>>,
}

impl Default for StoredConfig {
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
            filters: None,
        }
    }
}

impl From<StoredConfig> for AppConfig {
    fn from(old: StoredConfig) -> Self {
        let filters = old.filters.unwrap_or_else(|| {
            vec![
                FilterConfig::new(
                    1,
                    FilterSettings::HighPass(HighPassConfig {
                        enabled: old.highpass_enabled,
                        cutoff_hz: old.highpass_hz,
                    }),
                ),
                FilterConfig::new(
                    2,
                    FilterSettings::Rnnoise(NoiseConfig {
                        enabled: old.rnnoise_enabled,
                    }),
                ),
                FilterConfig::new(3, FilterSettings::Expander(old.expander)),
                FilterConfig::new(4, FilterSettings::Compressor(old.compressor)),
                FilterConfig::new(
                    5,
                    FilterSettings::Gain(GainConfig {
                        enabled: true,
                        gain_db: old.master_gain_db,
                    }),
                ),
            ]
        });
        let mut cfg = Self {
            input_device: old.input_device,
            output_device: old.output_device,
            monitor_device: old.monitor_device,
            filters,
            monitor_enabled: old.monitor_enabled,
            monitor_gain_db: old.monitor_gain_db,
            start_processing: old.start_processing,
            start_minimized: old.start_minimized,
        };
        // Hand-edited configs may omit IDs or repeat them. Give every instance
        // a unique identity so processor state cannot be shared accidentally.
        let mut used = std::collections::HashSet::new();
        let mut next = cfg.filters.iter().map(|f| f.id).max().unwrap_or(0);
        for filter in &mut cfg.filters {
            if filter.id == 0 || !used.insert(filter.id) {
                loop {
                    next = next.wrapping_add(1);
                    if next != 0 && used.insert(next) {
                        filter.id = next;
                        break;
                    }
                }
            }
        }
        cfg
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default)]
    pub id: u64,
    #[serde(flatten)]
    pub settings: FilterSettings,
}

impl FilterConfig {
    pub fn new(id: u64, settings: FilterSettings) -> Self {
        Self { id, settings }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    HighPass,
    Rnnoise,
    Expander,
    Compressor,
    Gain,
}

impl FilterKind {
    pub const ALL: [Self; 5] = [
        Self::HighPass,
        Self::Rnnoise,
        Self::Expander,
        Self::Compressor,
        Self::Gain,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HighPass => "High-pass",
            Self::Rnnoise => "RNNoise",
            Self::Expander => "Expander",
            Self::Compressor => "Compressor",
            Self::Gain => "Gain",
        }
    }

    pub fn default_settings(self) -> FilterSettings {
        match self {
            Self::HighPass => FilterSettings::HighPass(HighPassConfig::default()),
            Self::Rnnoise => FilterSettings::Rnnoise(NoiseConfig::default()),
            Self::Expander => FilterSettings::Expander(ExpanderConfig::default()),
            Self::Compressor => FilterSettings::Compressor(CompressorConfig::default()),
            Self::Gain => FilterSettings::Gain(GainConfig::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FilterSettings {
    HighPass(HighPassConfig),
    Rnnoise(NoiseConfig),
    Expander(ExpanderConfig),
    Compressor(CompressorConfig),
    Gain(GainConfig),
}

impl FilterSettings {
    pub fn kind(&self) -> FilterKind {
        match self {
            Self::HighPass(_) => FilterKind::HighPass,
            Self::Rnnoise(_) => FilterKind::Rnnoise,
            Self::Expander(_) => FilterKind::Expander,
            Self::Compressor(_) => FilterKind::Compressor,
            Self::Gain(_) => FilterKind::Gain,
        }
    }

    pub fn enabled(&self) -> bool {
        *match self {
            Self::HighPass(c) => &c.enabled,
            Self::Rnnoise(c) => &c.enabled,
            Self::Expander(c) => &c.enabled,
            Self::Compressor(c) => &c.enabled,
            Self::Gain(c) => &c.enabled,
        }
    }

    pub fn enabled_mut(&mut self) -> &mut bool {
        match self {
            Self::HighPass(c) => &mut c.enabled,
            Self::Rnnoise(c) => &mut c.enabled,
            Self::Expander(c) => &mut c.enabled,
            Self::Compressor(c) => &mut c.enabled,
            Self::Gain(c) => &mut c.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighPassConfig {
    pub enabled: bool,
    pub cutoff_hz: f32,
}

impl Default for HighPassConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cutoff_hz: 80.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseConfig {
    pub enabled: bool,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GainConfig {
    pub enabled: bool,
    pub gain_db: f32,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gain_db: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Global atomics and ordered settings shared with the DSP thread.
/// Audio callbacks never access this mutex; DSP only uses try_lock.
pub struct LiveParams {
    pub filters: Mutex<Vec<FilterConfig>>,
    pub monitor_enabled: AtomicBool,
    pub monitor_gain_db: AtomicF32,
    pub muted: AtomicBool,
    pub running: AtomicBool,
}

impl LiveParams {
    pub fn from_config(cfg: &AppConfig) -> Arc<Self> {
        Arc::new(Self {
            filters: Mutex::new(cfg.filters.clone()),
            monitor_enabled: AtomicBool::new(cfg.monitor_enabled),
            monitor_gain_db: AtomicF32::new(cfg.monitor_gain_db),
            muted: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

    pub fn publish_filters(&self, filters: &[FilterConfig]) {
        let filters = filters.to_vec();
        *self.filters.lock() = filters;
    }

    pub fn snapshot_into(&self, cfg: &mut AppConfig) {
        cfg.filters = self.filters.lock().clone();
        cfg.monitor_enabled = self.monitor_enabled.load(Ordering::Relaxed);
        cfg.monitor_gain_db = self.monitor_gain_db.load(Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips_through_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let back: AppConfig = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, cfg);
        assert!(!text.contains("master_gain_db"));
    }

    #[test]
    fn live_params_mirror_config_and_snapshot() {
        let mut cfg = AppConfig::default();
        let live = LiveParams::from_config(&cfg);
        cfg.filters.reverse();
        cfg.filters.pop();
        live.publish_filters(&cfg.filters);
        live.monitor_gain_db.store(-1.25, Ordering::Relaxed);
        let mut out = AppConfig::default();
        live.snapshot_into(&mut out);
        assert_eq!(out.filters, cfg.filters);
        assert_eq!(out.monitor_gain_db, -1.25);
    }

    #[test]
    fn legacy_config_preserves_settings_and_order() {
        let cfg: AppConfig = toml::from_str(
            r#"
input_device = "Microphone"
output_device = "Cable"
monitor_device = "Headphones"
highpass_enabled = false
highpass_hz = 120.0
rnnoise_enabled = false
master_gain_db = 3.5
monitor_enabled = true
monitor_gain_db = -12.0
start_processing = false
start_minimized = true
[expander]
enabled = false
threshold_db = -33.0
[compressor]
makeup_db = 9.0
"#,
        )
        .unwrap();
        assert_eq!(
            cfg.filters
                .iter()
                .map(|f| f.settings.kind())
                .collect::<Vec<_>>(),
            FilterKind::ALL
        );
        assert_eq!(
            cfg.filters[0].settings,
            FilterSettings::HighPass(HighPassConfig {
                enabled: false,
                cutoff_hz: 120.0
            })
        );
        assert!(!cfg.filters[1].settings.enabled());
        if let FilterSettings::Expander(c) = &cfg.filters[2].settings {
            assert!(!c.enabled);
            assert_eq!(c.threshold_db, -33.0);
            assert_eq!(c.ratio, 3.0);
        } else {
            panic!("expected expander");
        }
        if let FilterSettings::Compressor(c) = &cfg.filters[3].settings {
            assert_eq!(c.makeup_db, 9.0);
        } else {
            panic!("expected compressor");
        }
        assert_eq!(
            cfg.filters[4].settings,
            FilterSettings::Gain(GainConfig {
                enabled: true,
                gain_db: 3.5
            })
        );
        assert_eq!(cfg.input_device.as_deref(), Some("Microphone"));
        assert_eq!(cfg.output_device.as_deref(), Some("Cable"));
        assert_eq!(cfg.monitor_device.as_deref(), Some("Headphones"));
        assert!(cfg.monitor_enabled && cfg.start_minimized);
        assert!(!cfg.start_processing);
        assert_eq!(cfg.monitor_gain_db, -12.0);
    }

    #[test]
    fn explicit_empty_chain_overrides_legacy_defaults() {
        let cfg: AppConfig = toml::from_str("filters = []\nmaster_gain_db = 20.0").unwrap();
        assert!(cfg.filters.is_empty());
        let back: AppConfig = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert!(back.filters.is_empty());
    }

    #[test]
    fn missing_and_duplicate_ids_are_normalized() {
        let cfg: AppConfig = toml::from_str(
            r#"
[[filters]]
type = "gain"
id = 1
gain_db = 3.0
[[filters]]
type = "gain"
id = 1
gain_db = -6.0
[[filters]]
type = "rnnoise"
"#,
        )
        .unwrap();
        let ids: std::collections::HashSet<_> = cfg.filters.iter().map(|f| f.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&0));
        assert_eq!(
            cfg.filters[0].settings.kind(),
            cfg.filters[1].settings.kind()
        );
        assert_eq!(
            toml::from_str::<AppConfig>(&toml::to_string_pretty(&cfg).unwrap()).unwrap(),
            cfg
        );
    }
}
