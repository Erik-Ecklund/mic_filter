/// Biquad high-pass filter (Direct Form I).
#[derive(Debug, Clone)]
pub struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    sample_rate: f32,
    cutoff_hz: f32,
}

impl HighPass {
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let mut hp = Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            sample_rate,
            // Force set_cutoff to recompute coefficients on first call.
            cutoff_hz: f32::NAN,
        };
        hp.set_cutoff(cutoff_hz);
        hp
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        let cutoff = cutoff_hz.clamp(20.0, self.sample_rate * 0.45);
        if self.cutoff_hz.is_finite() && (cutoff - self.cutoff_hz).abs() < 0.01 {
            return;
        }
        self.cutoff_hz = cutoff;

        // Butterworth high-pass, Q = 1/sqrt(2)
        let w0 = 2.0 * std::f32::consts::PI * cutoff / self.sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / std::f32::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 + cos_w0) * 0.5) / a0;
        self.b1 = (-(1.0 + cos_w0)) / a0;
        self.b2 = ((1.0 + cos_w0) * 0.5) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn process_block(&mut self, samples: &mut [f32]) {
        for s in samples {
            *s = self.process_sample(*s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(xs: &[f32]) -> f32 {
        let sum: f32 = xs.iter().map(|x| x * x).sum();
        (sum / xs.len() as f32).sqrt()
    }

    fn tone(freq_hz: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate;
                (2.0 * std::f32::consts::PI * freq_hz * t).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn attenuates_sub_cutoff_more_than_passband() {
        let sr = 48_000.0;
        let mut hp = HighPass::new(sr, 80.0);
        let mut low = tone(20.0, sr, 4800);
        hp.process_block(&mut low);

        let mut hp2 = HighPass::new(sr, 80.0);
        let mut high = tone(1000.0, sr, 4800);
        hp2.process_block(&mut high);

        let low_rms = rms(&low[2400..]);
        let high_rms = rms(&high[2400..]);
        assert!(
            high_rms > low_rms * 5.0,
            "expected passband >> stopband, high={high_rms} low={low_rms}"
        );
    }

    #[test]
    fn coefficients_initialized_on_new() {
        let hp = HighPass::new(48_000.0, 80.0);
        assert!(hp.cutoff_hz.is_finite());
        assert!((hp.b0 - 1.0).abs() > 0.01 || hp.b1.abs() > 0.01);
    }
}
