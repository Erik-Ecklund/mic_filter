use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::compressor::Compressor;
use super::expander::Expander;
use super::highpass::HighPass;
use super::noise_suppression::NoiseSuppressor;
use crate::config::LiveParams;

const PROCESS_SR: f32 = 48_000.0;

/// Ordered chain: HPF → RNNoise → Expander → Compressor → master gain.
pub struct DspChain {
    params: Arc<LiveParams>,
    highpass: HighPass,
    denoise: NoiseSuppressor,
    expander: Expander,
    compressor: Compressor,
    scratch: Vec<f32>,
    last_hp_hz: f32,
}

impl DspChain {
    pub fn new(params: Arc<LiveParams>) -> Self {
        let hp_hz = params.highpass_hz.load(Ordering::Relaxed);
        Self {
            params,
            highpass: HighPass::new(PROCESS_SR, hp_hz),
            denoise: NoiseSuppressor::new(),
            expander: Expander::new(PROCESS_SR),
            compressor: Compressor::new(PROCESS_SR),
            scratch: Vec::with_capacity(2048),
            last_hp_hz: hp_hz,
        }
    }

    /// Process mono ±1.0 samples in-place at 48 kHz.
    pub fn process(&mut self, samples: &mut [f32]) {
        let hp_hz = self.params.highpass_hz.load(Ordering::Relaxed);
        if (hp_hz - self.last_hp_hz).abs() > 0.1 {
            self.highpass.set_cutoff(hp_hz);
            self.last_hp_hz = hp_hz;
        }
        self.expander.set_params(
            self.params.expander_threshold_db.load(Ordering::Relaxed),
            self.params.expander_ratio.load(Ordering::Relaxed),
            self.params.expander_attack_ms.load(Ordering::Relaxed),
            self.params.expander_release_ms.load(Ordering::Relaxed),
            self.params.expander_hold_ms.load(Ordering::Relaxed),
            self.params.expander_range_db.load(Ordering::Relaxed),
        );
        self.compressor.set_params(
            self.params.compressor_threshold_db.load(Ordering::Relaxed),
            self.params.compressor_ratio.load(Ordering::Relaxed),
            self.params.compressor_attack_ms.load(Ordering::Relaxed),
            self.params.compressor_release_ms.load(Ordering::Relaxed),
            self.params.compressor_knee_db.load(Ordering::Relaxed),
            self.params.compressor_makeup_db.load(Ordering::Relaxed),
        );

        if self.params.highpass_enabled.load(Ordering::Relaxed) {
            self.highpass.process_block(samples);
        }

        if self.params.rnnoise_enabled.load(Ordering::Relaxed) {
            if self.scratch.len() < samples.len() {
                self.scratch.resize(samples.len(), 0.0);
            }
            self.denoise
                .process(samples, &mut self.scratch[..samples.len()]);
            samples.copy_from_slice(&self.scratch[..samples.len()]);
        }

        if self.params.expander_enabled.load(Ordering::Relaxed) {
            self.expander.process_block(samples);
        }

        if self.params.compressor_enabled.load(Ordering::Relaxed) {
            self.compressor.process_block(samples);
        }

        let master_db = self.params.master_gain_db.load(Ordering::Relaxed);
        if master_db.abs() > 0.01 {
            let g = 10.0f32.powf(master_db * 0.05);
            for s in samples.iter_mut() {
                *s = (*s * g).clamp(-1.0, 1.0);
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
    use crate::config::{AppConfig, LiveParams};
    use std::sync::atomic::Ordering;

    fn passthrough_params() -> Arc<LiveParams> {
        let params = LiveParams::from_config(&AppConfig::default());
        params.rnnoise_enabled.store(false, Ordering::Relaxed);
        params.highpass_enabled.store(false, Ordering::Relaxed);
        params.expander_enabled.store(false, Ordering::Relaxed);
        params.compressor_enabled.store(false, Ordering::Relaxed);
        params
    }

    #[test]
    fn mute_silences_output() {
        let params = passthrough_params();
        params.muted.store(true, Ordering::Relaxed);
        let mut chain = DspChain::new(params);
        let mut buf = vec![0.25f32; 480];
        chain.process(&mut buf);
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn master_gain_scales_passthrough() {
        let params = passthrough_params();
        params.master_gain_db.store(6.0, Ordering::Relaxed); // ×2
        let mut chain = DspChain::new(params);
        let mut buf = vec![0.25f32; 480];
        chain.process(&mut buf);
        let peak = buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak={peak}");
    }
}
