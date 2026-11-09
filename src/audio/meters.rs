use std::sync::atomic::Ordering;
use std::sync::Arc;

use atomic_float::AtomicF32;

#[derive(Default)]
pub struct LevelMeters {
    pub input_peak: AtomicF32,
    pub output_peak: AtomicF32,
    pub input_hold: AtomicF32,
    pub output_hold: AtomicF32,
}

impl LevelMeters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn push_input(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        self.store_peak(&self.input_peak, &self.input_hold, peak);
    }

    pub fn push_output(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        self.store_peak(&self.output_peak, &self.output_hold, peak);
    }

    fn store_peak(&self, peak_atom: &AtomicF32, hold_atom: &AtomicF32, peak: f32) {
        let prev = peak_atom.load(Ordering::Relaxed);
        peak_atom.store((prev * 0.92).max(peak), Ordering::Relaxed);
        let hold = hold_atom.load(Ordering::Relaxed);
        if peak >= hold {
            hold_atom.store(peak, Ordering::Relaxed);
        }
    }

    /// Called from the UI thread ~30 Hz.
    pub fn decay(&self) {
        let i = self.input_peak.load(Ordering::Relaxed) * 0.88;
        let o = self.output_peak.load(Ordering::Relaxed) * 0.88;
        self.input_peak.store(i, Ordering::Relaxed);
        self.output_peak.store(o, Ordering::Relaxed);

        // Peak hold falls more slowly.
        let ih = self.input_hold.load(Ordering::Relaxed) * 0.97;
        let oh = self.output_hold.load(Ordering::Relaxed) * 0.97;
        let ip = self.input_peak.load(Ordering::Relaxed);
        let op = self.output_peak.load(Ordering::Relaxed);
        self.input_hold.store(ih.max(ip), Ordering::Relaxed);
        self.output_hold.store(oh.max(op), Ordering::Relaxed);
    }

    pub fn input_db(&self) -> f32 {
        peak_to_db(self.input_peak.load(Ordering::Relaxed))
    }

    pub fn output_db(&self) -> f32 {
        peak_to_db(self.output_peak.load(Ordering::Relaxed))
    }

    pub fn input_hold_db(&self) -> f32 {
        peak_to_db(self.input_hold.load(Ordering::Relaxed))
    }

    pub fn output_hold_db(&self) -> f32 {
        peak_to_db(self.output_hold.load(Ordering::Relaxed))
    }
}

pub fn peak_to_db(peak: f32) -> f32 {
    if peak < 1e-8 {
        -60.0
    } else {
        (20.0 * peak.log10()).clamp(-60.0, 0.0)
    }
}

/// Map dBFS in [-60, 0] to 0..1 for meter drawing.
pub fn db_to_meter(db: f32) -> f32 {
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
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
        assert!((meters.input_db() - (-60.0)).abs() < 1e-3);
    }

    #[test]
    fn db_to_meter_endpoints() {
        assert!((db_to_meter(-60.0) - 0.0).abs() < 1e-5);
        assert!((db_to_meter(0.0) - 1.0).abs() < 1e-5);
        assert!(db_to_meter(-12.0) > 0.7 && db_to_meter(-12.0) < 0.9);
    }
}
