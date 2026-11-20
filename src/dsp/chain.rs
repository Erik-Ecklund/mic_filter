use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::filter::{AudioFilter, create_filter};
use super::limiter::Limiter;
use crate::config::{FilterConfig, LimiterConfig, LiveParams};

struct FilterInstance {
    config: FilterConfig,
    processor: Box<dyn AudioFilter>,
}

/// An ordered collection of independent stateful processors.
pub struct DspChain {
    params: Arc<LiveParams>,
    filters: Vec<FilterInstance>,
    limiter_config: LimiterConfig,
    limiter: Limiter,
    replay: bool,
    generation: u64,
}

impl DspChain {
    pub fn new(params: Arc<LiveParams>) -> Self {
        Self::with_role(params, false)
    }

    pub fn new_replay(params: Arc<LiveParams>) -> Self {
        Self::with_role(params, true)
    }

    fn with_role(params: Arc<LiveParams>, replay: bool) -> Self {
        let config = params.chain.lock().clone();
        let mut chain = Self {
            params,
            filters: Vec::new(),
            limiter: Limiter::new(&config.limiter),
            limiter_config: config.limiter,
            replay,
            generation: config.generation,
        };
        chain.reconcile(config.filters);
        chain
    }

    fn reconcile(&mut self, config: Vec<FilterConfig>) {
        let mut previous = std::mem::take(&mut self.filters);
        self.filters = config
            .into_iter()
            .map(|config| {
                // Retain envelopes, biquad history, and RNNoise state across moves
                // and parameter edits. A changed type gets a fresh processor.
                if let Some(index) = previous.iter().position(|old| {
                    old.config.id == config.id
                        && old.config.settings.kind() == config.settings.kind()
                }) {
                    let mut instance = previous.remove(index);
                    if instance.config.settings != config.settings {
                        instance.processor.configure(&config.settings);
                    }
                    instance.config = config;
                    instance
                } else {
                    FilterInstance {
                        processor: create_filter(&config.settings),
                        config,
                    }
                }
            })
            .collect();
    }

    /// Process mono ±1.0 samples in-place at 48 kHz.
    pub fn process(&mut self, samples: &mut [f32]) {
        // Never wait for the UI. If it is publishing, keep the current chain
        // for this block and pick up the latest complete snapshot next time.
        let update = self.params.chain.try_lock().and_then(|config| {
            let unchanged = config.generation == self.generation
                && config.limiter == self.limiter_config
                && config.filters.len() == self.filters.len()
                && config
                    .filters
                    .iter()
                    .zip(&self.filters)
                    .all(|(a, b)| *a == b.config);
            (!unchanged).then(|| config.clone())
        });
        // Release the settings lock before constructing/configuring processors.
        if let Some(config) = update {
            if config.generation != self.generation {
                self.filters.clear();
                self.limiter = Limiter::new(&config.limiter);
                self.generation = config.generation;
            }
            self.limiter.configure(&config.limiter);
            self.limiter_config = config.limiter;
            self.reconcile(config.filters);
        }
        for filter in &mut self.filters {
            if filter.config.settings.enabled() {
                filter.processor.process(samples);
            }
        }
        let reduction = self.limiter.process(samples);
        let meter = if self.replay {
            &self.params.replay_limiter_reduction_db
        } else {
            &self.params.limiter_reduction_db
        };
        meter.store(reduction, Ordering::Relaxed);
        if self.params.muted.load(Ordering::Relaxed) {
            samples.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FilterKind, FilterSettings, GainConfig, HighPassConfig};

    #[test]
    fn new_filter_bypass_is_exact_and_limiter_still_runs_after_eq() {
        let mut settings = FilterKind::ALL
            .into_iter()
            .filter(|kind| {
                matches!(
                    kind,
                    FilterKind::ParametricEq | FilterKind::GraphicEq | FilterKind::DeEsser
                )
            })
            .map(|kind| kind.default_settings())
            .collect::<Vec<_>>();
        for setting in &mut settings {
            *setting.enabled_mut() = false;
        }
        let filters = settings
            .into_iter()
            .enumerate()
            .map(|(i, setting)| FilterConfig::new(i as u64 + 1, setting))
            .collect::<Vec<_>>();
        let shared = params(filters.clone());
        let mut chain = DspChain::new(shared.clone());
        let mut samples = crate::dsp::test_support::tone(6000.0, 0.3, 480);
        let original = samples.clone();
        chain.process(&mut samples);
        assert_eq!(samples, original);
        let mut eq = crate::config::ParametricEqConfig::default();
        eq.bands[0].frequency_hz = 6000.0;
        eq.bands[0].gain_db = 18.0;
        shared.publish_chain(
            &[FilterConfig::new(3, FilterSettings::ParametricEq(eq))],
            &LimiterConfig {
                enabled: true,
                ceiling_db: -6.0,
                release_ms: 100.0,
            },
        );
        for _ in 0..10 {
            samples.copy_from_slice(&original);
            chain.process(&mut samples);
            assert!(
                samples
                    .iter()
                    .all(|s| s.abs() <= super::super::math::db_to_lin(-6.0))
            );
        }
    }

    #[test]
    fn eq_and_de_esser_keep_history_on_moves_and_parameter_edits() {
        for kind in [
            FilterKind::ParametricEq,
            FilterKind::GraphicEq,
            FilterKind::DeEsser,
        ] {
            let mut settings = kind.default_settings();
            if let FilterSettings::ParametricEq(config) = &mut settings {
                config.bands[0].gain_db = 9.0;
            }
            if let FilterSettings::GraphicEq(config) = &mut settings {
                config.gains_db[5] = 9.0;
            }
            if let FilterSettings::DeEsser(config) = &mut settings {
                config.threshold_db = -40.0;
            }
            let filter = FilterConfig::new(1, settings.clone());
            let shared = params(vec![filter.clone(), gain(2, 0.0)]);
            let reference_params = params(vec![filter]);
            let mut chain = DspChain::new(shared.clone());
            let mut reference = DspChain::new(reference_params.clone());
            let original = crate::dsp::test_support::tone(6000.0, 0.3, 480);
            for _ in 0..20 {
                let mut a = original.clone();
                let mut b = original.clone();
                chain.process(&mut a);
                reference.process(&mut b);
                assert_eq!(a, b);
            }
            if let FilterSettings::ParametricEq(config) = &mut settings {
                config.bands[0].frequency_hz = 2000.0;
            }
            if let FilterSettings::GraphicEq(config) = &mut settings {
                config.gains_db[5] = 3.0;
            }
            if let FilterSettings::DeEsser(config) = &mut settings {
                config.threshold_db = -30.0;
            }
            let edited = FilterConfig::new(1, settings);
            shared.publish_filters(&[gain(2, 0.0), edited.clone()]);
            reference_params.publish_filters(&[edited]);
            let mut a = original.clone();
            let mut b = original;
            chain.process(&mut a);
            reference.process(&mut b);
            assert_eq!(a, b);
        }
    }

    #[test]
    fn limiter_runs_after_gain_and_live_ceiling_changes_apply() {
        let params = params(vec![gain(1, 6.0)]);
        let mut limiter = LimiterConfig {
            enabled: true,
            ceiling_db: -12.0,
            release_ms: 100.0,
        };
        params.publish_chain(&[gain(1, 6.0)], &limiter);
        let mut chain = DspChain::new(params.clone());
        let mut samples = [0.25; 480];
        chain.process(&mut samples);
        let ceiling = super::super::math::db_to_lin(-12.0);
        assert!(samples.iter().all(|s| s.abs() <= ceiling));
        assert!((samples[0] - ceiling).abs() < 1e-6);
        assert!(params.limiter_reduction_db.load(Ordering::Relaxed) > 5.0);
        limiter.ceiling_db = -18.0;
        params.publish_chain(&[gain(1, 6.0)], &limiter);
        samples.fill(0.25);
        chain.process(&mut samples);
        assert!(
            samples
                .iter()
                .all(|s| s.abs() <= super::super::math::db_to_lin(-18.0))
        );
        params.muted.store(true, Ordering::Relaxed);
        chain.process(&mut samples);
        assert_eq!(samples, [0.0; 480]);
    }

    #[test]
    fn replay_has_independent_limiter_envelope_and_meter() {
        let params = params(vec![]);
        params.publish_chain(
            &[],
            &LimiterConfig {
                enabled: true,
                ceiling_db: -6.0,
                release_ms: 100.0,
            },
        );
        let mut live = DspChain::new(params.clone());
        let mut replay = DspChain::new_replay(params.clone());
        live.process(&mut [4.0; 480]);
        let live_reduction = params.limiter_reduction_db.load(Ordering::Relaxed);
        let mut quiet = [0.1; 480];
        replay.process(&mut quiet);
        assert_eq!(quiet, [0.1; 480]);
        assert_eq!(
            params.replay_limiter_reduction_db.load(Ordering::Relaxed),
            0.0
        );
        assert_eq!(
            params.limiter_reduction_db.load(Ordering::Relaxed),
            live_reduction
        );
        assert!(live_reduction > 10.0);
    }

    #[test]
    fn loading_even_an_identical_empty_profile_resets_limiter_state() {
        let params = params(vec![]);
        let limiter = LimiterConfig {
            enabled: true,
            ..LimiterConfig::default()
        };
        params.publish_chain(&[], &limiter);
        let mut chain = DspChain::new(params.clone());
        chain.process(&mut [10.0; 480]);
        let mut quiet = [0.1; 480];
        chain.process(&mut quiet);
        assert!(quiet[0] < 0.1);
        params.publish_profile(&[], &limiter);
        quiet.fill(0.1);
        chain.process(&mut quiet);
        assert_eq!(quiet, [0.1; 480]);
    }

    #[test]
    fn contended_profile_snapshot_keeps_old_chain_and_limiter_together() {
        let params = params(vec![]);
        let mut chain = DspChain::new(params.clone());
        let mut guard = params.chain.lock();
        guard.filters = vec![gain(1, 20.0)];
        guard.limiter = LimiterConfig {
            enabled: true,
            ceiling_db: -20.0,
            release_ms: 100.0,
        };
        let mut samples = [0.5; 480];
        chain.process(&mut samples);
        assert_eq!(samples, [0.5; 480]);
        drop(guard);
        samples.fill(0.5);
        chain.process(&mut samples);
        assert!(
            samples
                .iter()
                .all(|s| s.abs() <= super::super::math::db_to_lin(-20.0))
        );
    }

    fn gain(id: u64, db: f32) -> FilterConfig {
        FilterConfig::new(
            id,
            FilterSettings::Gain(GainConfig {
                enabled: true,
                gain_db: db,
            }),
        )
    }

    fn params(filters: Vec<FilterConfig>) -> Arc<LiveParams> {
        LiveParams::from_config(&AppConfig {
            filters,
            ..AppConfig::default()
        })
    }

    #[test]
    fn empty_chain_passes_through_and_mute_still_silences() {
        let params = params(vec![]);
        let mut chain = DspChain::new(params.clone());
        let mut buf = [0.25, -0.5, 0.0];
        chain.process(&mut buf);
        assert_eq!(buf, [0.25, -0.5, 0.0]);
        params.muted.store(true, Ordering::Relaxed);
        chain.process(&mut buf);
        assert_eq!(buf, [0.0; 3]);
    }

    #[test]
    fn gain_scales_passthrough() {
        let mut chain = DspChain::new(params(vec![gain(1, 6.0)]));
        let mut buf = [0.25; 480];
        chain.process(&mut buf);
        assert!((buf[0] - 0.5).abs() < 0.02);
    }

    #[test]
    fn bypassed_filters_are_passthrough() {
        let mut cfg = AppConfig::default();
        for f in &mut cfg.filters {
            *f.settings.enabled_mut() = false;
        }
        let mut chain = DspChain::new(LiveParams::from_config(&cfg));
        let mut buf = [0.25; 480];
        chain.process(&mut buf);
        assert_eq!(buf, [0.25; 480]);
    }

    #[test]
    fn repeated_filters_have_independent_settings_and_order_matters() {
        // Boost first clips at unity, then attenuation yields 0.1.
        // Reversing these independent gains avoids clipping and yields 0.5.
        let params = params(vec![gain(1, 20.0), gain(2, -20.0)]);
        let mut chain = DspChain::new(params.clone());
        let mut buf = [0.5; 480];
        chain.process(&mut buf);
        assert!((buf[0] - 0.1).abs() < 1e-5);
        params.publish_filters(&[gain(2, -20.0), gain(1, 20.0)]);
        buf.fill(0.5);
        chain.process(&mut buf);
        assert!((buf[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn live_add_remove_and_parameter_changes() {
        let params = params(vec![]);
        let mut chain = DspChain::new(params.clone());
        for (filters, expected) in [
            (vec![gain(1, 20.0)], 1.0),
            (vec![gain(1, -20.0)], 0.05),
            (vec![gain(1, -20.0), gain(2, 6.0)], 0.05 * 10.0f32.powf(0.3)),
            (vec![], 0.5),
        ] {
            params.publish_filters(&filters);
            let mut buf = [0.5; 480];
            chain.process(&mut buf);
            assert!((buf[0] - expected).abs() < 1e-5);
        }
    }

    #[test]
    fn busy_publisher_does_not_block_processing() {
        let params = params(vec![gain(1, -20.0)]);
        let mut chain = DspChain::new(params.clone());
        let mut guard = params.chain.lock();
        guard.filters.clear();
        let mut buf = [0.5; 480];
        chain.process(&mut buf);
        assert!((buf[0] - 0.05).abs() < 1e-5);
        drop(guard);
        buf.fill(0.5);
        chain.process(&mut buf);
        assert_eq!(buf, [0.5; 480]);
    }

    #[test]
    fn moving_and_editing_filter_preserves_signal_history() {
        let hp = FilterConfig::new(1, FilterSettings::HighPass(HighPassConfig::default()));
        let params = params(vec![hp.clone(), gain(2, 0.0)]);
        let mut chain = DspChain::new(params.clone());
        let mut reference = DspChain::new(super::tests::params(vec![hp.clone()]));
        let mut a = [0.25; 480];
        let mut b = a;
        chain.process(&mut a);
        reference.process(&mut b);
        // This cutoff change should keep the biquad's history.
        let edited = FilterConfig::new(
            1,
            FilterSettings::HighPass(HighPassConfig {
                cutoff_hz: 100.0,
                ..HighPassConfig::default()
            }),
        );
        params.publish_filters(&[gain(2, 0.0), edited.clone()]);
        reference.params.publish_filters(&[edited]);
        a.fill(0.25);
        b.fill(0.25);
        chain.process(&mut a);
        reference.process(&mut b);
        assert_eq!(a, b);
        assert!(a[0].abs() < 0.1, "a reset biquad would pass the onset");
    }

    #[test]
    fn default_chain_matches_original_processing_sequence() {
        use crate::dsp::{
            compressor::Compressor, expander::Expander, highpass::HighPass,
            noise_suppression::NoiseSuppressor,
        };
        let cfg = AppConfig::default();
        assert_eq!(
            cfg.filters
                .iter()
                .map(|f| f.settings.kind())
                .collect::<Vec<_>>(),
            FilterKind::DEFAULT_CHAIN
        );
        let mut chain = DspChain::new(LiveParams::from_config(&cfg));
        let mut hp = HighPass::new(48_000.0, 80.0);
        let mut ns = NoiseSuppressor::new();
        let mut exp = Expander::new(48_000.0);
        let mut comp = Compressor::new(48_000.0);
        for frame in 0..4 {
            let mut actual: Vec<_> = (0..480)
                .map(|i| ((i + frame * 480) as f32 * 0.03).sin() * 0.2)
                .collect();
            let mut expected = actual.clone();
            hp.process_block(&mut expected);
            let mut scratch = vec![0.0; 480];
            ns.process(&expected, &mut scratch);
            expected.copy_from_slice(&scratch);
            exp.process_block(&mut expected);
            comp.process_block(&mut expected);
            chain.process(&mut actual);
            assert_eq!(actual, expected);
        }
    }
}
