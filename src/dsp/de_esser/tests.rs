use super::*;
use crate::dsp::test_support::{amplitude_at, rms, tone};

fn config() -> DeEsserConfig {
    DeEsserConfig {
        threshold_db: -30.0,
        ratio: 10.0,
        max_reduction_db: 12.0,
        ..DeEsserConfig::default()
    }
}

#[test]
fn reduces_sibilant_band_without_ducking_low_speech() {
    let process = |frequency| {
        let mut de_esser = DeEsser::new(&config());
        let mut samples = tone(frequency, 0.5, 48_000);
        de_esser.process_block(&mut samples);
        rms(&samples[24_000..]) / (0.5 / 2.0_f32.sqrt())
    };
    let high = process(6000.0);
    let low = process(300.0);
    assert!(high < 0.35, "sibilant ratio {high}");
    assert!(low > 0.98, "low speech ratio {low}");
}

#[test]
fn mixed_voice_and_sibilance_preserve_low_frequency_component() {
    let mut samples = tone(6000.0, 0.3, 48_000);
    for (sample, voice) in samples.iter_mut().zip(tone(300.0, 0.3, 48_000)) {
        *sample += voice;
    }
    let mut de_esser = DeEsser::new(&config());
    de_esser.process_block(&mut samples);
    let speech = amplitude_at(&samples[24_000..], 300.0);
    let sibilance = amplitude_at(&samples[24_000..], 6000.0);
    assert!((speech - 0.3).abs() < 0.01, "speech {speech}");
    assert!(sibilance < 0.12, "sibilance {sibilance}");
}

#[test]
fn below_threshold_is_exact_passthrough() {
    let mut de_esser = DeEsser::new(&DeEsserConfig {
        threshold_db: 0.0,
        ..config()
    });
    let mut samples = tone(6000.0, 0.1, 4800);
    let original = samples.clone();
    de_esser.process_block(&mut samples);
    assert_eq!(samples, original);
}

#[test]
fn ratio_one_and_zero_range_bypass_even_after_live_edits() {
    for (ratio, range) in [(1.0, 12.0), (10.0, 0.0)] {
        let mut config = config();
        let mut de_esser = DeEsser::new(&config);
        de_esser.process_block(&mut tone(6000.0, 0.5, 4800));
        assert!(de_esser.reduction_db > 1.0);
        config.ratio = ratio;
        config.max_reduction_db = range;
        de_esser.configure(&config);
        let mut samples = tone(6000.0, 0.5, 4800);
        let original = samples.clone();
        de_esser.process_block(&mut samples);
        assert_eq!(samples, original);
    }
}

#[test]
fn attack_and_release_are_smooth_and_maximum_reduction_is_bounded() {
    let mut de_esser = DeEsser::new(&config());
    de_esser.process_block(&mut tone(6000.0, 0.5, 48));
    let early = de_esser.reduction_db;
    de_esser.process_block(&mut tone(6000.0, 0.5, 48_000));
    let engaged = de_esser.reduction_db;
    assert!(engaged > early + 3.0);
    assert!(engaged <= 12.0 + 1e-8);
    de_esser.process_block(&mut [0.0; 48]);
    assert!(de_esser.reduction_db > 1.0);
    de_esser.process_block(&mut vec![0.0; 96_000]);
    assert!(de_esser.reduction_db < 0.01);
}

#[test]
fn changing_detection_frequency_targets_the_new_band() {
    let mut config = config();
    config.q = 8.0;
    config.frequency_hz = 3000.0;
    let mut de_esser = DeEsser::new(&config);
    let mut samples = tone(3000.0, 0.4, 48_000);
    de_esser.process_block(&mut samples);
    assert!(rms(&samples[24_000..]) < 0.15);
    config.frequency_hz = 9000.0;
    de_esser.configure(&config);
    let mut samples = tone(3000.0, 0.4, 96_000);
    de_esser.process_block(&mut samples);
    assert!(rms(&samples[72_000..]) > 0.27);
}

#[test]
fn invalid_values_are_sanitized_and_output_recovers() {
    let config = DeEsserConfig {
        frequency_hz: f32::NAN,
        q: f32::NEG_INFINITY,
        threshold_db: f32::NAN,
        ratio: 0.0,
        attack_ms: f32::INFINITY,
        release_ms: -1.0,
        max_reduction_db: f32::INFINITY,
        ..config()
    };
    let mut de_esser = DeEsser::new(&config);
    let mut invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, 0.1];
    de_esser.process_block(&mut invalid);
    assert!(invalid.iter().all(|s| s.is_finite()));
    de_esser.process_block(&mut vec![0.0; 48_000]);
    let mut normal = tone(1000.0, 0.1, 4800);
    de_esser.process_block(&mut normal);
    assert!(normal.iter().all(|s| s.is_finite()));
}
