use super::biquad::Biquad;
use crate::config::ParametricEqConfig;

pub(super) struct ParametricEq {
    bands: [Biquad; 3],
    enabled: [bool; 3],
}

impl ParametricEq {
    pub fn new(config: &ParametricEqConfig) -> Self {
        let mut eq = Self {
            bands: std::array::from_fn(|_| Biquad::new()),
            enabled: [true; 3],
        };
        eq.configure(config);
        eq
    }

    pub fn configure(&mut self, config: &ParametricEqConfig) {
        for (index, band) in config.bands.iter().enumerate() {
            self.enabled[index] = band.enabled;
            self.bands[index].set_peaking(band.frequency_hz, band.gain_db, band.q);
        }
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for (band, enabled) in self.bands.iter_mut().zip(self.enabled) {
            if enabled {
                for sample in samples.iter_mut() {
                    *sample = band.process_sample(*sample);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
