use super::math::finite_clamp;

const SAMPLE_RATE: f64 = 48_000.0;

/// Direct Form I with f64 coefficients/history to retain precision at low
/// frequencies and high Q. Coefficient edits do not discard signal history.
pub(super) struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    pub fn new() -> Self {
        Self {
            b: [1.0, 0.0, 0.0],
            a: [0.0; 2],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn set_coefficients(&mut self, b: [f64; 3], a: [f64; 3]) {
        self.b = b.map(|value| value / a[0]);
        self.a = [a[1] / a[0], a[2] / a[0]];
    }

    pub fn set_peaking(&mut self, frequency_hz: f32, gain_db: f32, q: f32) {
        let frequency = finite_clamp(frequency_hz, 20.0, 20_000.0, 1000.0) as f64;
        let gain = finite_clamp(gain_db, -18.0, 18.0, 0.0) as f64;
        let q = finite_clamp(q, 0.1, 12.0, 1.0) as f64;
        if gain == 0.0 {
            self.set_coefficients([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
            return;
        }
        // RBJ peaking EQ, where Q is the bell bandwidth control.
        let w = std::f64::consts::TAU * frequency / SAMPLE_RATE;
        let alpha = w.sin() / (2.0 * q);
        let amplitude = 10.0_f64.powf(gain / 40.0);
        self.set_coefficients(
            [
                1.0 + alpha * amplitude,
                -2.0 * w.cos(),
                1.0 - alpha * amplitude,
            ],
            [
                1.0 + alpha / amplitude,
                -2.0 * w.cos(),
                1.0 - alpha / amplitude,
            ],
        );
    }

    pub fn set_bandpass(&mut self, frequency_hz: f32, q: f32) {
        let frequency = finite_clamp(frequency_hz, 2000.0, 12_000.0, 6000.0) as f64;
        let q = finite_clamp(q, 0.5, 8.0, 2.0) as f64;
        let w = std::f64::consts::TAU * frequency / SAMPLE_RATE;
        let alpha = w.sin() / (2.0 * q);
        // Constant 0 dB peak gain: band amplitude is calibrated at its center.
        self.set_coefficients(
            [alpha, 0.0, -alpha],
            [1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha],
        );
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        let input = if input.is_finite() { input as f64 } else { 0.0 };
        let output = self.b[0] * input + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        if !(output as f32).is_finite() {
            self.x = [0.0; 2];
            self.y = [0.0; 2];
            return 0.0;
        }
        let output = if output.abs() < 1e-30 { 0.0 } else { output };
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output as f32
    }
}
