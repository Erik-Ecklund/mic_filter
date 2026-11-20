use super::biquad::Biquad;
use super::math::{finite_clamp, time_to_coeff};
use crate::config::DeEsserConfig;

/// Band-selective compression: subtract only the attenuated part of a
/// unity-peak band-pass signal from the dry audio, instead of ducking all speech.
pub(super) struct DeEsser {
    bandpass: Biquad,
    envelope: f64,
    reduction_db: f64,
    threshold_db: f64,
    ratio: f64,
    max_reduction_db: f64,
    attack: f64,
    release: f64,
}

impl DeEsser {
    pub fn new(config: &DeEsserConfig) -> Self {
        let mut de_esser = Self {
            bandpass: Biquad::new(),
            envelope: 0.0,
            reduction_db: 0.0,
            threshold_db: -24.0,
            ratio: 4.0,
            max_reduction_db: 9.0,
            attack: 0.0,
            release: 0.0,
        };
        de_esser.configure(config);
        de_esser
    }

    pub fn configure(&mut self, config: &DeEsserConfig) {
        self.bandpass.set_bandpass(config.frequency_hz, config.q);
        self.threshold_db = finite_clamp(config.threshold_db, -60.0, 0.0, -24.0) as f64;
        self.ratio = finite_clamp(config.ratio, 1.0, 20.0, 4.0) as f64;
        self.max_reduction_db = finite_clamp(config.max_reduction_db, 0.0, 18.0, 9.0) as f64;
        let attack = finite_clamp(config.attack_ms, 0.1, 20.0, 2.0);
        let release = finite_clamp(config.release_ms, 10.0, 300.0, 80.0);
        self.attack = time_to_coeff(attack, 48_000.0) as f64;
        self.release = time_to_coeff(release, 48_000.0) as f64;
        self.reduction_db = self.reduction_db.min(self.max_reduction_db);
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for sample in samples {
            let input = if sample.is_finite() { *sample } else { 0.0 };
            let band = self.bandpass.process_sample(input) as f64;
            let level = band.abs();
            let coefficient = if level > self.envelope {
                self.attack
            } else {
                self.release
            };
            self.envelope = coefficient * self.envelope + (1.0 - coefficient) * level;
            let over_db = (20.0 * self.envelope.max(1e-12).log10() - self.threshold_db).max(0.0);
            let target = (over_db * (1.0 - 1.0 / self.ratio)).min(self.max_reduction_db);
            let coefficient = if target > self.reduction_db {
                self.attack
            } else {
                self.release
            };
            self.reduction_db = coefficient * self.reduction_db + (1.0 - coefficient) * target;
            // Ratio 1 or zero range is exact dry audio, even after a live edit.
            let gain = if self.ratio == 1.0 || self.max_reduction_db == 0.0 {
                1.0
            } else {
                10.0_f64.powf(-self.reduction_db / 20.0)
            };
            let output = (input as f64 - (1.0 - gain) * band) as f32;
            *sample = if output.is_finite() { output } else { 0.0 };
        }
    }
}

#[cfg(test)]
mod tests;
