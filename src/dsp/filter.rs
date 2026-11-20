use super::compressor::Compressor;
use super::de_esser::DeEsser;
use super::expander::Expander;
use super::graphic_eq::GraphicEq;
use super::highpass::HighPass;
use super::math::db_to_lin;
use super::noise_suppression::NoiseSuppressor;
use super::parametric_eq::ParametricEq;
use crate::config::FilterSettings;

const PROCESS_SR: f32 = 48_000.0;

/// A stateful, in-place mono processor at 48 kHz.
/// The chain handles identity, ordering, and bypass; filters own only their DSP.
pub trait AudioFilter {
    /// Called on construction and when this instance's settings change.
    fn configure(&mut self, settings: &FilterSettings);
    fn process(&mut self, samples: &mut [f32]);
}

impl AudioFilter for ParametricEq {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::ParametricEq(config) = settings {
            ParametricEq::configure(self, config);
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

impl AudioFilter for DeEsser {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::DeEsser(config) = settings {
            DeEsser::configure(self, config);
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

impl AudioFilter for GraphicEq {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::GraphicEq(config) = settings {
            GraphicEq::configure(self, config);
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

/// The only place where the chain needs to know the built-in processor types.
pub fn create_filter(settings: &FilterSettings) -> Box<dyn AudioFilter> {
    let mut filter: Box<dyn AudioFilter> = match settings {
        FilterSettings::HighPass(c) => Box::new(HighPass::new(PROCESS_SR, c.cutoff_hz)),
        FilterSettings::Rnnoise(_) => Box::new(NoiseFilter {
            denoise: NoiseSuppressor::new(),
            scratch: vec![0.0; 480],
        }),
        FilterSettings::Expander(_) => Box::new(Expander::new(PROCESS_SR)),
        FilterSettings::Compressor(_) => Box::new(Compressor::new(PROCESS_SR)),
        FilterSettings::Gain(_) => Box::new(Gain { db: 0.0 }),
        FilterSettings::ParametricEq(c) => Box::new(ParametricEq::new(c)),
        FilterSettings::GraphicEq(c) => Box::new(GraphicEq::new(c)),
        FilterSettings::DeEsser(c) => Box::new(DeEsser::new(c)),
    };
    filter.configure(settings);
    filter
}

impl AudioFilter for HighPass {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::HighPass(c) = settings {
            self.set_cutoff(c.cutoff_hz);
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

impl AudioFilter for Expander {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::Expander(c) = settings {
            self.set_params(
                c.threshold_db,
                c.ratio,
                c.attack_ms,
                c.release_ms,
                c.hold_ms,
                c.range_db,
            );
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

impl AudioFilter for Compressor {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::Compressor(c) = settings {
            self.set_params(
                c.threshold_db,
                c.ratio,
                c.attack_ms,
                c.release_ms,
                c.knee_db,
                c.makeup_db,
            );
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        self.process_block(samples);
    }
}

struct NoiseFilter {
    denoise: NoiseSuppressor,
    scratch: Vec<f32>,
}

impl AudioFilter for NoiseFilter {
    fn configure(&mut self, _settings: &FilterSettings) {}

    fn process(&mut self, samples: &mut [f32]) {
        if self.scratch.len() < samples.len() {
            self.scratch.resize(samples.len(), 0.0);
        }
        self.denoise
            .process(samples, &mut self.scratch[..samples.len()]);
        samples.copy_from_slice(&self.scratch[..samples.len()]);
    }
}

struct Gain {
    db: f32,
}

impl AudioFilter for Gain {
    fn configure(&mut self, settings: &FilterSettings) {
        if let FilterSettings::Gain(c) = settings {
            self.db = c.gain_db;
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        // Preserve the original master gain's unity bypass and clipping behavior.
        if self.db.abs() > 0.01 {
            let gain = db_to_lin(self.db);
            for sample in samples {
                *sample = (*sample * gain).clamp(-1.0, 1.0);
            }
        }
    }
}
