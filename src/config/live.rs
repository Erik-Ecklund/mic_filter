use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atomic_float::AtomicF32;
use parking_lot::Mutex;

use super::{AppConfig, FilterConfig};

/// Global atomics and ordered settings shared with the DSP thread.
/// Audio callbacks never access this mutex; DSP only uses try_lock.
pub struct LiveParams {
    pub tuning: crate::audio::Tuning,
    pub filters: Mutex<Vec<FilterConfig>>,
    pub monitor_enabled: AtomicBool,
    pub monitor_gain_db: AtomicF32,
    pub muted: AtomicBool,
    pub running: AtomicBool,
}

impl LiveParams {
    pub fn from_config(cfg: &AppConfig) -> Arc<Self> {
        Arc::new(Self {
            tuning: crate::audio::Tuning::new(),
            filters: Mutex::new(cfg.filters.clone()),
            monitor_enabled: AtomicBool::new(cfg.monitor_enabled),
            monitor_gain_db: AtomicF32::new(cfg.monitor_gain_db),
            muted: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

    pub fn publish_filters(&self, filters: &[FilterConfig]) {
        let filters = filters.to_vec();
        *self.filters.lock() = filters;
    }

    pub fn snapshot_into(&self, cfg: &mut AppConfig) {
        cfg.filters = self.filters.lock().clone();
        cfg.monitor_enabled = self.monitor_enabled.load(Ordering::Relaxed);
        cfg.monitor_gain_db = self.monitor_gain_db.load(Ordering::Relaxed);
    }
}
