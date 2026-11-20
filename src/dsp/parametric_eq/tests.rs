use super::*;
use crate::dsp::test_support::{rms, tone};

fn config(gain_db: f32, q: f32) -> ParametricEqConfig {
    let mut config = ParametricEqConfig::default();
    config.bands[0].frequency_hz = 1000.0;
    config.bands[0].gain_db = gain_db;
    config.bands[0].q = q;
    config
}

#[test]
fn flat_eq_is_exact_passthrough() {
    let mut eq = ParametricEq::new(&ParametricEqConfig::default());
    let mut samples = tone(300.0, 0.2, 4800);
    let original = samples.clone();
    eq.process_block(&mut samples);
    assert_eq!(samples, original);
}

#[test]
fn center_frequency_gain_and_cut_match_requested_db() {
    for gain_db in [-18.0, -6.0, 6.0, 18.0] {
        let mut eq = ParametricEq::new(&config(gain_db, 2.0));
        let mut samples = tone(1000.0, 0.05, 24_000);
        eq.process_block(&mut samples);
        let ratio = rms(&samples[12_000..]) / (0.05 / 2.0_f32.sqrt());
        assert!(
            (20.0 * ratio.log10() - gain_db).abs() < 0.02,
            "{gain_db}: {ratio}"
        );
    }
}

#[test]
fn higher_q_is_narrower_and_multiple_bands_are_independent() {
    let response = |q| {
        let mut eq = ParametricEq::new(&config(12.0, q));
        let mut samples = tone(1800.0, 0.05, 24_000);
        eq.process_block(&mut samples);
        rms(&samples[12_000..])
    };
    assert!(response(0.5) > response(8.0) * 1.5);
    let mut config = config(6.0, 1.0);
    config.bands[1] = config.bands[0].clone();
    config.bands[1].enabled = false;
    let mut single = ParametricEq::new(&config);
    let mut a = tone(1000.0, 0.05, 24_000);
    single.process_block(&mut a);
    config.bands[1].enabled = true;
    let mut double = ParametricEq::new(&config);
    let mut b = tone(1000.0, 0.05, 24_000);
    double.process_block(&mut b);
    let ratio = rms(&b[12_000..]) / rms(&a[12_000..]);
    assert!((20.0 * ratio.log10() - 6.0).abs() < 0.02);
}

#[test]
fn reconfiguration_retains_signal_history() {
    let mut config = config(6.0, 1.0);
    let mut eq = ParametricEq::new(&config);
    eq.process_block(&mut [0.25]);
    config.bands[0].frequency_hz = 2000.0;
    eq.configure(&config);
    let mut tail = [0.0; 480];
    eq.process_block(&mut tail);
    assert!(tail.iter().any(|sample| sample.abs() > 1e-4));
    let mut fresh = ParametricEq::new(&config);
    let mut silence = [0.0; 480];
    fresh.process_block(&mut silence);
    assert_eq!(silence, [0.0; 480]);
}

#[test]
fn extreme_valid_settings_and_impulse_tails_remain_stable() {
    for frequency in [20.0, 20_000.0] {
        for q in [0.1, 12.0] {
            for gain in [-18.0, 18.0] {
                let mut config = config(gain, q);
                config.bands[0].frequency_hz = frequency;
                let mut eq = ParametricEq::new(&config);
                let mut impulse = vec![0.0; 96_000];
                impulse[0] = 0.1;
                eq.process_block(&mut impulse);
                assert!(impulse.iter().all(|s| s.is_finite() && s.abs() < 1.0));
                assert!(rms(&impulse[95_000..]) < 1e-4);
            }
        }
    }
}

#[test]
fn invalid_parameters_and_samples_do_not_poison_history() {
    let mut config = config(f32::NAN, f32::INFINITY);
    config.bands[0].frequency_hz = f32::NEG_INFINITY;
    let mut eq = ParametricEq::new(&config);
    let mut samples = [f32::NAN, f32::INFINITY, 0.2, -0.3, 0.0];
    eq.process_block(&mut samples);
    assert!(samples.iter().all(|s| s.is_finite()));
    eq.configure(&self::config(6.0, 1.0));
    eq.process_block(&mut [f32::MAX; 100]);
    let mut recovery = tone(1000.0, 0.1, 4800);
    eq.process_block(&mut recovery);
    assert!(recovery.iter().all(|s| s.is_finite()));
}
