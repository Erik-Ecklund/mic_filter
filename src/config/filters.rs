use serde::{Deserialize, Serialize};

pub const GRAPHIC_EQ_FREQUENCIES: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16_000.0,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicEqConfig {
    pub enabled: bool,
    pub preamp_db: f32,
    pub gains_db: [f32; 10],
}

impl Default for GraphicEqConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            gains_db: [0.0; 10],
        }
    }
}

impl GraphicEqConfig {
    pub fn reset_flat(&mut self) {
        self.preamp_db = 0.0;
        self.gains_db.fill(0.0);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimiterConfig {
    pub enabled: bool,
    pub ceiling_db: f32,
    pub release_ms: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqBandConfig {
    pub enabled: bool,
    pub frequency_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for EqBandConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency_hz: 1000.0,
            gain_db: 0.0,
            q: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParametricEqConfig {
    pub enabled: bool,
    pub bands: [EqBandConfig; 3],
}

impl Default for ParametricEqConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bands: [150.0, 1000.0, 5000.0].map(|frequency_hz| EqBandConfig {
                frequency_hz,
                ..EqBandConfig::default()
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeEsserConfig {
    pub enabled: bool,
    pub frequency_hz: f32,
    pub q: f32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub max_reduction_db: f32,
}

impl Default for DeEsserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency_hz: 6000.0,
            q: 2.0,
            threshold_db: -24.0,
            ratio: 4.0,
            attack_ms: 2.0,
            release_ms: 80.0,
            max_reduction_db: 9.0,
        }
    }
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ceiling_db: -1.0,
            release_ms: 100.0,
        }
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
    ParametricEq,
    GraphicEq,
    DeEsser,
}

impl FilterKind {
    #[cfg(test)]
    pub const DEFAULT_CHAIN: [Self; 5] = [
        Self::HighPass,
        Self::Rnnoise,
        Self::Expander,
        Self::Compressor,
        Self::Gain,
    ];

    pub const ALL: [Self; 8] = [
        Self::HighPass,
        Self::Rnnoise,
        Self::Expander,
        Self::Compressor,
        Self::Gain,
        Self::ParametricEq,
        Self::GraphicEq,
        Self::DeEsser,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HighPass => "High-pass",
            Self::Rnnoise => "RNNoise",
            Self::Expander => "Expander",
            Self::Compressor => "Compressor",
            Self::Gain => "Gain",
            Self::ParametricEq => "Parametric EQ",
            Self::GraphicEq => "Graphic EQ",
            Self::DeEsser => "De-esser",
        }
    }

    pub fn default_settings(self) -> FilterSettings {
        match self {
            Self::HighPass => FilterSettings::HighPass(HighPassConfig::default()),
            Self::Rnnoise => FilterSettings::Rnnoise(NoiseConfig::default()),
            Self::Expander => FilterSettings::Expander(ExpanderConfig::default()),
            Self::Compressor => FilterSettings::Compressor(CompressorConfig::default()),
            Self::Gain => FilterSettings::Gain(GainConfig::default()),
            Self::ParametricEq => FilterSettings::ParametricEq(ParametricEqConfig::default()),
            Self::GraphicEq => FilterSettings::GraphicEq(GraphicEqConfig::default()),
            Self::DeEsser => FilterSettings::DeEsser(DeEsserConfig::default()),
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
    ParametricEq(ParametricEqConfig),
    GraphicEq(GraphicEqConfig),
    DeEsser(DeEsserConfig),
}

impl FilterSettings {
    pub fn kind(&self) -> FilterKind {
        match self {
            Self::HighPass(_) => FilterKind::HighPass,
            Self::Rnnoise(_) => FilterKind::Rnnoise,
            Self::Expander(_) => FilterKind::Expander,
            Self::Compressor(_) => FilterKind::Compressor,
            Self::Gain(_) => FilterKind::Gain,
            Self::ParametricEq(_) => FilterKind::ParametricEq,
            Self::GraphicEq(_) => FilterKind::GraphicEq,
            Self::DeEsser(_) => FilterKind::DeEsser,
        }
    }

    pub fn enabled(&self) -> bool {
        *match self {
            Self::HighPass(c) => &c.enabled,
            Self::Rnnoise(c) => &c.enabled,
            Self::Expander(c) => &c.enabled,
            Self::Compressor(c) => &c.enabled,
            Self::Gain(c) => &c.enabled,
            Self::ParametricEq(c) => &c.enabled,
            Self::GraphicEq(c) => &c.enabled,
            Self::DeEsser(c) => &c.enabled,
        }
    }

    pub fn enabled_mut(&mut self) -> &mut bool {
        match self {
            Self::HighPass(c) => &mut c.enabled,
            Self::Rnnoise(c) => &mut c.enabled,
            Self::Expander(c) => &mut c.enabled,
            Self::Compressor(c) => &mut c.enabled,
            Self::Gain(c) => &mut c.enabled,
            Self::ParametricEq(c) => &mut c.enabled,
            Self::GraphicEq(c) => &mut c.enabled,
            Self::DeEsser(c) => &mut c.enabled,
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
