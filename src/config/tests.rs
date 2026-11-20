use super::*;
use std::sync::atomic::Ordering;

#[test]
fn graphic_eq_defaults_and_flat_reset_preserve_bypass() {
    let filter: FilterConfig = toml::from_str("type = 'graphic_eq'").unwrap();
    assert_eq!(
        filter.settings,
        FilterSettings::GraphicEq(GraphicEqConfig::default())
    );
    let mut config = GraphicEqConfig {
        enabled: false,
        preamp_db: -9.0,
        gains_db: [6.0; 10],
    };
    config.reset_flat();
    assert!(!config.enabled);
    assert_eq!(config.preamp_db, 0.0);
    assert_eq!(config.gains_db, [0.0; 10]);
    assert_eq!(AppConfig::default().filters.len(), 5);
}

#[test]
fn every_available_filter_roundtrips_and_supports_bypass() {
    for (index, kind) in FilterKind::ALL.into_iter().enumerate() {
        let mut filter = FilterConfig::new(index as u64 + 1, kind.default_settings());
        assert_eq!(filter.settings.kind(), kind);
        assert!(filter.settings.enabled());
        *filter.settings.enabled_mut() = false;
        assert!(!filter.settings.enabled());
        let text = toml::to_string_pretty(&filter).unwrap();
        let back: FilterConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, filter);
    }
}

#[test]
fn minimal_eq_and_de_esser_settings_have_defaults_without_changing_old_chain() {
    let eq: FilterConfig = toml::from_str("type = 'parametric_eq'").unwrap();
    assert_eq!(
        eq.settings,
        FilterSettings::ParametricEq(ParametricEqConfig::default())
    );
    let de_esser: FilterConfig = toml::from_str("type = 'de_esser'").unwrap();
    assert_eq!(
        de_esser.settings,
        FilterSettings::DeEsser(DeEsserConfig::default())
    );
    let old: AppConfig = toml::from_str("").unwrap();
    assert_eq!(old.filters.len(), 5);
    assert!(!old.filters.iter().any(|filter| matches!(
        filter.settings,
        FilterSettings::ParametricEq(_) | FilterSettings::DeEsser(_)
    )));
}

#[test]
fn default_config_roundtrips_through_toml() {
    let cfg = AppConfig::default();
    let text = toml::to_string_pretty(&cfg).expect("serialize");
    let back: AppConfig = toml::from_str(&text).expect("deserialize");
    assert_eq!(back, cfg);
    assert!(!text.contains("master_gain_db"));
}

#[test]
fn live_params_mirror_config_and_snapshot() {
    let mut cfg = AppConfig::default();
    let live = LiveParams::from_config(&cfg);
    cfg.filters.reverse();
    cfg.filters.pop();
    live.publish_filters(&cfg.filters);
    live.monitor_gain_db.store(-1.25, Ordering::Relaxed);
    cfg.limiter.enabled = true;
    cfg.limiter.ceiling_db = -4.0;
    live.publish_chain(&cfg.filters, &cfg.limiter);
    let mut out = AppConfig::default();
    live.snapshot_into(&mut out);
    assert_eq!(out.filters, cfg.filters);
    assert_eq!(out.monitor_gain_db, -1.25);
    assert_eq!(out.limiter, cfg.limiter);
}

#[test]
fn legacy_config_preserves_settings_and_order() {
    let cfg: AppConfig = toml::from_str(
        r#"
input_device = "Microphone"
output_device = "Cable"
monitor_device = "Headphones"
highpass_enabled = false
highpass_hz = 120.0
rnnoise_enabled = false
master_gain_db = 3.5
monitor_enabled = true
monitor_gain_db = -12.0
start_processing = false
start_minimized = true
[expander]
enabled = false
threshold_db = -33.0
[compressor]
makeup_db = 9.0
"#,
    )
    .unwrap();
    assert_eq!(
        cfg.filters
            .iter()
            .map(|f| f.settings.kind())
            .collect::<Vec<_>>(),
        FilterKind::DEFAULT_CHAIN
    );
    assert_eq!(
        cfg.filters[0].settings,
        FilterSettings::HighPass(HighPassConfig {
            enabled: false,
            cutoff_hz: 120.0
        })
    );
    assert!(!cfg.filters[1].settings.enabled());
    if let FilterSettings::Expander(c) = &cfg.filters[2].settings {
        assert!(!c.enabled);
        assert_eq!(c.threshold_db, -33.0);
        assert_eq!(c.ratio, 3.0);
    } else {
        panic!("expected expander");
    }
    if let FilterSettings::Compressor(c) = &cfg.filters[3].settings {
        assert_eq!(c.makeup_db, 9.0);
    } else {
        panic!("expected compressor");
    }
    assert_eq!(
        cfg.filters[4].settings,
        FilterSettings::Gain(GainConfig {
            enabled: true,
            gain_db: 3.5
        })
    );
    assert_eq!(cfg.input_device.as_deref(), Some("Microphone"));
    assert_eq!(cfg.output_device.as_deref(), Some("Cable"));
    assert_eq!(cfg.monitor_device.as_deref(), Some("Headphones"));
    assert!(cfg.monitor_enabled && cfg.start_minimized);
    assert!(!cfg.start_processing);
    assert_eq!(cfg.monitor_gain_db, -12.0);
}

#[test]
fn explicit_empty_chain_overrides_legacy_defaults() {
    let cfg: AppConfig = toml::from_str("filters = []\nmaster_gain_db = 20.0").unwrap();
    assert!(cfg.filters.is_empty());
    let back: AppConfig = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
    assert!(back.filters.is_empty());
}

#[test]
fn missing_and_duplicate_ids_are_normalized() {
    let cfg: AppConfig = toml::from_str(
        r#"
[[filters]]
type = "gain"
id = 1
gain_db = 3.0
[[filters]]
type = "gain"
id = 1
gain_db = -6.0
[[filters]]
type = "rnnoise"
"#,
    )
    .unwrap();
    let ids: std::collections::HashSet<_> = cfg.filters.iter().map(|f| f.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&0));
    assert_eq!(
        cfg.filters[0].settings.kind(),
        cfg.filters[1].settings.kind()
    );
    assert_eq!(
        toml::from_str::<AppConfig>(&toml::to_string_pretty(&cfg).unwrap()).unwrap(),
        cfg
    );
}
