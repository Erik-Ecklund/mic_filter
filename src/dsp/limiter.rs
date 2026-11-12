use super::math::{db_to_lin, lin_to_db, time_to_coeff};
use crate::config::LimiterConfig;

/// Zero-lookahead sample-peak limiter: instantaneous attack, smoothed recovery.
/// This bounds 48 kHz samples, not reconstructed/inter-sample peaks.
pub(super) struct Limiter {
    enabled: bool,
    ceiling: f32,
    release: f32,
    gain: f32,
}

impl Limiter {
    pub fn new(config: &LimiterConfig) -> Self {
        let mut limiter = Self {
            enabled: false,
            ceiling: 1.0,
            release: 0.0,
            gain: 1.0,
        };
        limiter.configure(config);
        limiter
    }

    pub fn configure(&mut self, config: &LimiterConfig) {
        if config.enabled != self.enabled {
            self.gain = 1.0;
        }
        self.enabled = config.enabled;
        let ceiling = if config.ceiling_db.is_finite() {
            config.ceiling_db
        } else {
            -1.0
        };
        let release = if config.release_ms.is_finite() {
            config.release_ms
        } else {
            100.0
        };
        self.ceiling = db_to_lin(ceiling.clamp(-24.0, 0.0));
        self.release = time_to_coeff(release.clamp(10.0, 1000.0), 48_000.0);
    }

    /// Return the maximum gain reduction in this block, in positive dB.
    pub fn process(&mut self, samples: &mut [f32]) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let mut minimum_gain = 1.0_f32;
        for sample in samples {
            let input = if sample.is_finite() { *sample } else { 0.0 };
            let required = if input.abs() > self.ceiling {
                self.ceiling / input.abs()
            } else {
                1.0
            };
            let recovered = self.release * self.gain + (1.0 - self.release);
            self.gain = required.min(recovered);
            minimum_gain = minimum_gain.min(self.gain);
            *sample = (input * self.gain).clamp(-self.ceiling, self.ceiling);
        }
        -lin_to_db(minimum_gain.max(1e-12))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LimiterConfig {
        LimiterConfig {
            enabled: true,
            ceiling_db: -6.0,
            release_ms: 100.0,
        }
    }

    #[test]
    fn contains_first_transient_both_polarities_and_extreme_samples() {
        let mut limiter = Limiter::new(&config());
        let ceiling = db_to_lin(-6.0);
        let mut samples = [2.0, -4.0, 0.2, 10.0, -f32::MAX, f32::MAX];
        let reduction = limiter.process(&mut samples);
        assert!(reduction > 6.0);
        assert_eq!(samples[0], ceiling);
        assert_eq!(samples[1], -ceiling);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= ceiling));
    }

    #[test]
    fn bypass_is_exact_and_release_recovers_smoothly() {
        let mut limiter = Limiter::new(&LimiterConfig::default());
        let mut samples = [0.2, -0.5, 2.0];
        assert_eq!(limiter.process(&mut samples), 0.0);
        assert_eq!(samples, [0.2, -0.5, 2.0]);
        limiter.configure(&config());
        limiter.process(&mut [5.0]);
        let mut quiet = [0.1; 480];
        limiter.process(&mut quiet);
        assert!(quiet[0] < quiet[479] && quiet[479] < 0.1);
        for _ in 0..200 {
            limiter.process(&mut [0.0; 480]);
        }
        let mut recovered = [0.1];
        limiter.process(&mut recovered);
        assert!((recovered[0] - 0.1).abs() < 0.001);
    }

    #[test]
    fn parameter_changes_contain_peaks_and_invalid_values_remain_finite() {
        let mut limiter = Limiter::new(&config());
        limiter.configure(&LimiterConfig {
            enabled: true,
            ceiling_db: -12.0,
            release_ms: 20.0,
        });
        let mut samples = [0.8, -0.8];
        limiter.process(&mut samples);
        assert!(samples.iter().all(|s| s.abs() <= db_to_lin(-12.0)));
        limiter.configure(&LimiterConfig {
            enabled: true,
            ceiling_db: f32::NAN,
            release_ms: f32::INFINITY,
        });
        let mut invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.2];
        assert!(limiter.process(&mut invalid).is_finite());
        assert!(invalid.iter().all(|s| s.is_finite()));
        assert_eq!(&invalid[..3], &[0.0; 3]);
    }
}
