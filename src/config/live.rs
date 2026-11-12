use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atomic_float::AtomicF32;
use parking_lot::Mutex;

use super::{AppConfig, FilterConfig, LimiterConfig};

#[derive(Clone, PartialEq)]
pub struct ChainSettings {
    pub filters: Vec<FilterConfig>,
    pub limiter: LimiterConfig,
    pub generation: u64,
}

/// Global atomics and ordered settings shared with the DSP thread.
/// Audio callbacks never access this mutex; DSP only uses try_lock.
pub struct LiveParams {
    pub tuning: crate::audio::Tuning,
    pub chain: Mutex<ChainSettings>,
    pub limiter_reduction_db: AtomicF32,
    pub replay_limiter_reduction_db: AtomicF32,
    pub monitor_enabled: AtomicBool,
    pub monitor_gain_db: AtomicF32,
    pub muted: AtomicBool,
    pub running: AtomicBool,
}

impl LiveParams {
    pub fn from_config(cfg: &AppConfig) -> Arc<Self> {
        Arc::new(Self {
            tuning: crate::audio::Tuning::new(),
            chain: Mutex::new(ChainSettings {
                filters: cfg.filters.clone(),
                limiter: cfg.limiter.clone(),
                generation: 0,
            }),
            limiter_reduction_db: AtomicF32::new(0.0),
            replay_limiter_reduction_db: AtomicF32::new(0.0),
            monitor_enabled: AtomicBool::new(cfg.monitor_enabled),
            monitor_gain_db: AtomicF32::new(cfg.monitor_gain_db),
            muted: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

    pub fn publish_filters(&self, filters: &[FilterConfig]) {
        let filters = filters.to_vec();
        self.chain.lock().filters = filters;
    }

    pub fn publish_chain(&self, filters: &[FilterConfig], limiter: &LimiterConfig) {
        self.publish_settings(filters, limiter, false);
    }

    pub fn publish_profile(&self, filters: &[FilterConfig], limiter: &LimiterConfig) {
        self.publish_settings(filters, limiter, true);
    }

    fn publish_settings(&self, filters: &[FilterConfig], limiter: &LimiterConfig, reset: bool) {
        let filters = filters.to_vec();
        let limiter = limiter.clone();
        let mut chain = self.chain.lock();
        chain.filters = filters;
        chain.limiter = limiter;
        if reset {
            chain.generation = chain.generation.wrapping_add(1);
        }
    }

    pub fn snapshot_into(&self, cfg: &mut AppConfig) {
        let chain = self.chain.lock();
        cfg.filters = chain.filters.clone();
        cfg.limiter = chain.limiter.clone();
        cfg.monitor_enabled = self.monitor_enabled.load(Ordering::Relaxed);
        cfg.monitor_gain_db = self.monitor_gain_db.load(Ordering::Relaxed);
    }
}
