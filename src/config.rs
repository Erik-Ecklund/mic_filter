use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod filters;
mod legacy;
mod live;
mod profiles;

pub use filters::*;
use legacy::StoredConfig;
pub use live::LiveParams;
pub use profiles::{Profile, ProfileDocument};

const APP_DIR: &str = "mic-processor";
const CONFIG_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredConfig")]
pub struct AppConfig {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub monitor_device: Option<String>,
    pub filters: Vec<FilterConfig>,
    pub limiter: LimiterConfig,
    pub profiles: Vec<Profile>,
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

#[cfg(test)]
mod tests;
