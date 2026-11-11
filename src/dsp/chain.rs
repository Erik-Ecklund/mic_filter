use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::filter::{AudioFilter, create_filter};
use crate::config::{FilterConfig, LiveParams};

struct FilterInstance {
    config: FilterConfig,
    processor: Box<dyn AudioFilter>,
}

/// An ordered collection of independent stateful processors.
pub struct DspChain {
    params: Arc<LiveParams>,
    filters: Vec<FilterInstance>,
}

impl DspChain {
    pub fn new(params: Arc<LiveParams>) -> Self {
        let config = params.filters.lock().clone();
        let mut chain = Self {
            params,
            filters: Vec::new(),
        };
        chain.reconcile(config);
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
        let update = self.params.filters.try_lock().and_then(|config| {
            let unchanged = config.len() == self.filters.len()
                && config
                    .iter()
                    .zip(&self.filters)
                    .all(|(a, b)| *a == b.config);
            (!unchanged).then(|| config.clone())
        });
        // Release the settings lock before constructing/configuring processors.
        if let Some(config) = update {
            self.reconcile(config);
        }

        for filter in &mut self.filters {
            if filter.config.settings.enabled() {
                filter.processor.process(samples);
            }
        }
        if self.params.muted.load(Ordering::Relaxed) {
            samples.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FilterKind, FilterSettings, GainConfig, HighPassConfig};

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
        let mut guard = params.filters.lock();
        guard.clear();
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
            FilterKind::ALL
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
