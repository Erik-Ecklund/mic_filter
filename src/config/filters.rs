use serde::{Deserialize, Serialize};

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
