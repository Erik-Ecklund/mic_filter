use super::*;
use crate::dsp::test_support::{rms, tone};

#[test]
fn flat_defaults_are_exact_passthrough() {
    let mut eq = GraphicEq::new(&GraphicEqConfig::default());
    let mut samples = tone(440.0, 0.25, 4800);
    let original = samples.clone();
    eq.process_block(&mut samples);
    assert_eq!(samples, original);
}

#[test]
fn every_band_boosts_and_cuts_at_its_fixed_center() {
    for (index, frequency) in GRAPHIC_EQ_FREQUENCIES.into_iter().enumerate() {
        for gain_db in [-12.0, 12.0] {
            let mut config = GraphicEqConfig::default();
            config.gains_db[index] = gain_db;
            let mut eq = GraphicEq::new(&config);
            let mut samples = tone(frequency, 0.05, 48_000);
            let input_rms = rms(&samples[24_000..]);
            eq.process_block(&mut samples);
            let measured = 20.0 * (rms(&samples[24_000..]) / input_rms).log10();
            assert!(
                (measured - gain_db).abs() < 0.05,
                "{frequency} Hz: {measured} vs {gain_db}"
            );
        }
    }
}

#[test]
fn distant_bands_are_preserved_and_preamp_leaves_headroom_without_clipping() {
    let mut config = GraphicEqConfig::default();
    config.gains_db[5] = 12.0;
    let mut eq = GraphicEq::new(&config);
    let mut samples = tone(63.0, 0.1, 48_000);
    let before = rms(&samples[24_000..]);
    eq.process_block(&mut samples);
    assert!((20.0 * (rms(&samples[24_000..]) / before).log10()).abs() < 0.1);
    config.reset_flat();
    config.preamp_db = -6.0;
    eq.configure(&config);
    let mut quiet = tone(1000.0, 0.1, 4800);
    let before = quiet.clone();
    eq.process_block(&mut quiet);
    for (actual, input) in quiet.iter().zip(before) {
        assert!((*actual - input * db_to_lin(-6.0)).abs() < 1e-6);
    }
    config.preamp_db = 12.0;
    eq.configure(&config);
    let mut loud = tone(1000.0, 0.5, 4800);
    eq.process_block(&mut loud);
    assert!(
        loud.iter().any(|s| s.abs() > 1.5),
        "EQ must not hard-clip boosts"
    );
}

#[test]
fn live_band_changes_keep_signal_history_and_flat_reset_is_neutral() {
    let mut config = GraphicEqConfig::default();
    config.gains_db[5] = 9.0;
    let mut eq = GraphicEq::new(&config);
    eq.process_block(&mut [0.25]);
    config.gains_db[5] = 3.0;
    eq.configure(&config);
    let mut tail = [0.0; 480];
    eq.process_block(&mut tail);
    assert!(tail.iter().any(|sample| sample.abs() > 1e-4));
    config.reset_flat();
    eq.configure(&config);
    let mut samples = tone(1000.0, 0.25, 480);
    let original = samples.clone();
    eq.process_block(&mut samples);
    assert_eq!(samples, original);
}

#[test]
fn simultaneous_extreme_gains_remain_stable_and_state_survives_block_boundaries() {
    for gain in [-12.0, 12.0] {
        let config = GraphicEqConfig {
            gains_db: [gain; 10],
            ..GraphicEqConfig::default()
        };
        let mut whole = GraphicEq::new(&config);
        let mut blocked = GraphicEq::new(&config);
        let mut samples = tone(1000.0, 0.01, 48_000);
        samples[0] = 0.1;
        let mut reference = samples.clone();
        whole.process_block(&mut reference);
        for block in samples.chunks_mut(137) {
            blocked.process_block(block);
        }
        assert_eq!(samples, reference);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() < 10.0));
        whole.process_block(&mut vec![0.0; 48_000]);
        let mut tail = [0.0; 480];
        whole.process_block(&mut tail);
        assert!(rms(&tail) < 1e-4);
    }
}

#[test]
fn invalid_config_values_and_input_do_not_poison_state() {
    let config = GraphicEqConfig {
        preamp_db: f32::NAN,
        gains_db: [
            f32::INFINITY,
            f32::NAN,
            -1000.0,
            1000.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
        ..GraphicEqConfig::default()
    };
    let mut eq = GraphicEq::new(&config);
    let mut samples = [f32::NAN, f32::INFINITY, -0.25, 0.5];
    eq.process_block(&mut samples);
    assert!(samples.iter().all(|s| s.is_finite()));
    let mut normal = tone(1000.0, 0.1, 4800);
    eq.process_block(&mut normal);
    assert!(normal.iter().all(|s| s.is_finite()));
}
