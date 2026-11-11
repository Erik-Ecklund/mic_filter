use super::math::{db_to_lin, lin_to_db, time_to_coeff};

/// Soft-knee feed-forward compressor with envelope detector.
#[derive(Debug, Clone)]
pub struct Compressor {
    sample_rate: f32,
    envelope: f32,
    gain: f32,
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    makeup_lin: f32,
    attack_coeff: f32,
    release_coeff: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut c = Self {
            sample_rate,
            envelope: 0.0,
            gain: 1.0,
            threshold_db: -18.0,
            ratio: 3.0,
            knee_db: 6.0,
            makeup_lin: 1.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
        };
        c.set_params(-18.0, 3.0, 10.0, 100.0, 6.0, 6.0);
        c
    }

    pub fn set_params(
        &mut self,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        knee_db: f32,
        makeup_db: f32,
    ) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
        self.knee_db = knee_db.max(0.0);
        self.makeup_lin = db_to_lin(makeup_db);
        self.attack_coeff = time_to_coeff(attack_ms, self.sample_rate);
        self.release_coeff = time_to_coeff(release_ms, self.sample_rate);
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
        let gr_db = soft_knee_gain_reduction(env_db, self.threshold_db, self.ratio, self.knee_db);
        let target_gain = db_to_lin(gr_db);

        if target_gain < self.gain {
            self.gain = self.attack_coeff * self.gain + (1.0 - self.attack_coeff) * target_gain;
        } else {
            self.gain = self.release_coeff * self.gain + (1.0 - self.release_coeff) * target_gain;
        }

        x * self.gain * self.makeup_lin
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for s in samples {
            *s = self.process_sample(*s);
        }
    }
}

/// Returns gain reduction in dB (≤ 0).
#[inline]
fn soft_knee_gain_reduction(level_db: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let half_knee = knee * 0.5;
    if level_db < threshold - half_knee {
        0.0
    } else if level_db > threshold + half_knee {
        let over = level_db - threshold;
        over * (1.0 / ratio - 1.0)
    } else if knee > 0.0 {
        let over = level_db - threshold + half_knee;
        (1.0 / ratio - 1.0) * (over * over) / (2.0 * knee)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_knee_no_reduction_below_threshold() {
        let gr = soft_knee_gain_reduction(-40.0, -18.0, 4.0, 6.0);
        assert_eq!(gr, 0.0);
    }

    #[test]
    fn soft_knee_hard_region_matches_ratio() {
        // 12 dB over threshold with ratio 4 => GR = 12 * (1/4 - 1) = -9
        let gr = soft_knee_gain_reduction(-6.0, -18.0, 4.0, 0.0);
        assert!((gr - (-9.0)).abs() < 1e-4, "gr={gr}");
    }

    #[test]
    fn compresses_loud_signal() {
        let mut comp = Compressor::new(48_000.0);
        comp.set_params(-20.0, 4.0, 0.1, 0.1, 0.0, 0.0);

        let loud = 0.8f32;
        let mut out = 0.0;
        for _ in 0..8000 {
            out = comp.process_sample(loud);
        }
        assert!(
            out.abs() < loud,
            "compressor should reduce loud signal: out={out}"
        );
        assert!(out.abs() > 0.05, "should not silence speech entirely");
    }

    #[test]
    fn makeup_gain_boosts_output() {
        let mut plain = Compressor::new(48_000.0);
        plain.set_params(-18.0, 3.0, 1.0, 1.0, 0.0, 0.0);
        let mut boosted = Compressor::new(48_000.0);
        boosted.set_params(-18.0, 3.0, 1.0, 1.0, 0.0, 12.0);

        let x = 0.05f32;
        let mut a = 0.0;
        let mut b = 0.0;
        for _ in 0..4000 {
            a = plain.process_sample(x);
            b = boosted.process_sample(x);
        }
        assert!(b.abs() > a.abs() * 2.0, "makeup should raise level");
    }
}
