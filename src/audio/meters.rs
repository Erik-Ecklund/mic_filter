use std::sync::atomic::Ordering;
use std::sync::Arc;

use atomic_float::AtomicF32;

#[derive(Default)]
pub struct LevelMeters {
    pub input_peak: AtomicF32,
    pub output_peak: AtomicF32,
}

impl LevelMeters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn push_input(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        let prev = self.input_peak.load(Ordering::Relaxed);
        self.input_peak
            .store((prev * 0.9).max(peak), Ordering::Relaxed);
    }

    pub fn push_output(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        let prev = self.output_peak.load(Ordering::Relaxed);
        self.output_peak
            .store((prev * 0.9).max(peak), Ordering::Relaxed);
    }

    pub fn decay(&self) {
        let i = self.input_peak.load(Ordering::Relaxed) * 0.85;
        let o = self.output_peak.load(Ordering::Relaxed) * 0.85;
        self.input_peak.store(i, Ordering::Relaxed);
        self.output_peak.store(o, Ordering::Relaxed);
    }

    pub fn input_db(&self) -> f32 {
        peak_to_db(self.input_peak.load(Ordering::Relaxed))
    }

    pub fn output_db(&self) -> f32 {
        peak_to_db(self.output_peak.load(Ordering::Relaxed))
    }
}

fn peak_to_db(peak: f32) -> f32 {
    if peak < 1e-8 {
        -80.0
    } else {
        (20.0 * peak.log10()).clamp(-80.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn peak_meters_track_and_decay() {
        let meters = LevelMeters::new();
        meters.push_input(&[0.0, 0.5, -0.25]);
        assert!(meters.input_peak.load(Ordering::Relaxed) >= 0.5 - 1e-6);
        let db = meters.input_db();
        assert!(db > -20.0 && db <= 0.0, "db={db}");

        meters.decay();
        assert!(meters.input_peak.load(Ordering::Relaxed) < 0.5);
    }

    #[test]
    fn silence_reads_as_floor_db() {
        let meters = LevelMeters::new();
        assert!((meters.input_db() - (-80.0)).abs() < 1e-3);
    }
}
