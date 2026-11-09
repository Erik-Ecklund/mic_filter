/// Downward expander with peak detector, hold, and range limit.
#[derive(Debug, Clone)]
pub struct Expander {
    sample_rate: f32,
    envelope: f32,
    gain: f32,
    hold_samples_left: u32,
    threshold_db: f32,
    ratio: f32,
    range_db: f32,
    attack_coeff: f32,
    release_coeff: f32,
    hold_samples: u32,
}

impl Expander {
    pub fn new(sample_rate: f32) -> Self {
        let mut e = Self {
            sample_rate,
            envelope: 0.0,
            gain: 1.0,
            hold_samples_left: 0,
            threshold_db: -40.0,
            ratio: 3.0,
            range_db: 20.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            hold_samples: 0,
        };
        e.set_params(-40.0, 3.0, 5.0, 80.0, 50.0, 20.0);
        e
    }

    pub fn set_params(
        &mut self,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        hold_ms: f32,
        range_db: f32,
    ) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
        self.range_db = range_db.max(0.0);
        self.attack_coeff = time_to_coeff(attack_ms, self.sample_rate);
        self.release_coeff = time_to_coeff(release_ms, self.sample_rate);
        self.hold_samples = ((hold_ms.max(0.0) * 0.001) * self.sample_rate).round() as u32;
    }

    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let level = x.abs();
        if level > self.envelope {
            self.envelope = self.attack_coeff * self.envelope + (1.0 - self.attack_coeff) * level;
        } else {
            self.envelope = self.release_coeff * self.envelope + (1.0 - self.release_coeff) * level;
        }

        let env_db = lin_to_db(self.envelope.max(1e-8));
        let mut target_gain_db = 0.0;
        if env_db < self.threshold_db {
            let delta = self.threshold_db - env_db;
            target_gain_db = -delta * (self.ratio - 1.0);
            target_gain_db = target_gain_db.max(-self.range_db);
        } else {
            self.hold_samples_left = self.hold_samples;
        }

        if self.hold_samples_left > 0 && env_db < self.threshold_db {
            // Hold open near the threshold after speech.
            if env_db >= self.threshold_db - 6.0 {
                target_gain_db = 0.0;
                self.hold_samples_left = self.hold_samples_left.saturating_sub(1);
            }
        }

        let target_gain = db_to_lin(target_gain_db);
        if target_gain < self.gain {
            self.gain = self.attack_coeff * self.gain + (1.0 - self.attack_coeff) * target_gain;
        } else {
            self.gain = self.release_coeff * self.gain + (1.0 - self.release_coeff) * target_gain;
        }

        x * self.gain
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for s in samples {
            *s = self.process_sample(*s);
        }
    }
}

#[inline]
fn time_to_coeff(ms: f32, sample_rate: f32) -> f32 {
    let t = (ms.max(0.01) * 0.001).max(1.0 / sample_rate);
    (-1.0 / (t * sample_rate)).exp()
}

#[inline]
fn lin_to_db(x: f32) -> f32 {
    20.0 * x.log10()
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db * 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_quiet_noise_below_threshold() {
        let mut exp = Expander::new(48_000.0);
        exp.set_params(-30.0, 4.0, 0.1, 0.1, 0.0, 40.0);

        let quiet = 0.001f32;
        let mut out_quiet = 0.0;
        for _ in 0..5000 {
            out_quiet = exp.process_sample(quiet);
        }
        assert!(
            out_quiet.abs() < quiet.abs(),
            "expander should attenuate below threshold: {out_quiet} vs {quiet}"
        );

        let loud = 0.5f32;
        let mut out_loud = 0.0;
        for _ in 0..5000 {
            out_loud = exp.process_sample(loud);
        }
        assert!(out_loud.abs() > out_quiet.abs() * 10.0);
        assert!((out_loud.abs() - loud).abs() < 0.05);
    }

    #[test]
    fn clamps_ratio_and_range() {
        let mut exp = Expander::new(48_000.0);
        exp.set_params(-40.0, 0.5, 5.0, 80.0, 0.0, -10.0);
        assert!(exp.ratio >= 1.0);
        assert!(exp.range_db >= 0.0);
    }
}
