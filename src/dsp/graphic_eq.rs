use super::biquad::Biquad;
use super::math::{db_to_lin, finite_clamp};
use crate::config::{GRAPHIC_EQ_FREQUENCIES, GraphicEqConfig};

/// Ten overlapping fixed-Q bell filters, approximately one octave apart.
/// All processing is in-place at 48 kHz, without lookahead or buffering.
pub(super) struct GraphicEq {
    bands: [Biquad; 10],
    preamp: f32,
}

impl GraphicEq {
    pub fn new(config: &GraphicEqConfig) -> Self {
        let mut eq = Self {
            bands: std::array::from_fn(|_| Biquad::new()),
            preamp: 1.0,
        };
        eq.configure(config);
        eq
    }

    pub fn configure(&mut self, config: &GraphicEqConfig) {
        self.preamp = db_to_lin(finite_clamp(config.preamp_db, -24.0, 12.0, 0.0));
        for ((band, frequency), gain) in self
            .bands
            .iter_mut()
            .zip(GRAPHIC_EQ_FREQUENCIES)
            .zip(config.gains_db)
        {
            band.set_peaking(
                frequency,
                finite_clamp(gain, -12.0, 12.0, 0.0),
                std::f32::consts::SQRT_2,
            );
        }
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for sample in samples.iter_mut() {
            *sample *= self.preamp;
        }
        for band in &mut self.bands {
            for sample in samples.iter_mut() {
                *sample = band.process_sample(*sample);
            }
        }
    }
}

#[cfg(test)]
mod tests;
