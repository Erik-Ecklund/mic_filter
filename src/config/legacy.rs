use serde::Deserialize;

use super::{AppConfig, filters::*};

/// Read both the original singleton settings and the new ordered list.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(super) struct StoredConfig {
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
