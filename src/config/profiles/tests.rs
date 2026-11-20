use super::*;
use crate::config::{FilterSettings, GainConfig};

#[test]
fn graphic_eq_profiles_preserve_every_band_and_preamp() {
    use crate::config::GraphicEqConfig;
    let eq = GraphicEqConfig {
        enabled: false,
        preamp_db: -6.0,
        gains_db: [-12.0, -9.0, -6.0, -3.0, 0.0, 3.0, 6.0, 9.0, 12.0, 1.5],
    };
    let mut config = AppConfig {
        filters: vec![FilterConfig::new(1, FilterSettings::GraphicEq(eq))],
        ..AppConfig::default()
    };
    config.save_new_profile("Graphic voice").unwrap();
    let text = ProfileDocument::export(&config.profiles[0]).unwrap();
    let profile = ProfileDocument::parse(&text).unwrap();
    assert_eq!(profile, config.profiles[0]);
    let back: AppConfig = toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
    assert_eq!(back, config);
}

#[test]
fn graphic_eq_imports_reject_wrong_band_count_and_invalid_gains() {
    for fields in [
        "preamp_db = nan",
        "preamp_db = 13.0",
        "preamp_db = -25.0",
        "gains_db = [0.0]",
        "gains_db = [nan, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]",
        "gains_db = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 13.0]",
    ] {
        let text = format!(
            "version = 1\n[profile]\nname = 'Invalid graphic'\n\
            [[profile.filters]]\ntype = 'graphic_eq'\n{fields}"
        );
        assert!(ProfileDocument::parse(&text).is_err(), "{fields}");
    }
}

#[test]
fn eq_and_de_esser_profiles_roundtrip_with_independent_bands_and_settings() {
    use crate::config::{DeEsserConfig, ParametricEqConfig};
    let mut eq = ParametricEqConfig::default();
    eq.bands[0].gain_db = -3.0;
    eq.bands[1].enabled = false;
    eq.bands[2].frequency_hz = 7000.0;
    eq.bands[2].q = 3.5;
    let de_esser = DeEsserConfig {
        threshold_db: -33.0,
        ..DeEsserConfig::default()
    };
    let mut cfg = AppConfig {
        filters: vec![
            FilterConfig::new(1, FilterSettings::ParametricEq(eq)),
            FilterConfig::new(2, FilterSettings::DeEsser(de_esser)),
        ],
        ..AppConfig::default()
    };
    cfg.save_new_profile("Voice").unwrap();
    let profile =
        ProfileDocument::parse(&ProfileDocument::export(&cfg.profiles[0]).unwrap()).unwrap();
    assert_eq!(profile, cfg.profiles[0]);
    let back: AppConfig = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
    assert_eq!(back, cfg);
}

#[test]
fn imports_reject_invalid_eq_bands_and_de_esser_parameters() {
    for (field, value) in [
        ("frequency_hz", "nan"),
        ("q", "0.0"),
        ("threshold_db", "-100.0"),
        ("ratio", "0.5"),
        ("attack_ms", "inf"),
        ("release_ms", "-1.0"),
        ("max_reduction_db", "99.0"),
    ] {
        let text = format!(
            "version = 1\n[profile]\nname = 'Bad'\n\
            [[profile.filters]]\ntype = 'de_esser'\n{field} = {value}"
        );
        assert!(ProfileDocument::parse(&text).is_err(), "{field}");
    }
    for (field, value) in [
        ("frequency_hz", "24000.0"),
        ("gain_db", "nan"),
        ("q", "-1.0"),
    ] {
        let text = format!(
            "version = 1\n[profile]\nname = 'Bad EQ'\n\
            [[profile.filters]]\ntype = 'parametric_eq'\n\
            bands = [{{{field} = {value}}}, {{}}, {{}}]"
        );
        assert!(ProfileDocument::parse(&text).is_err(), "{field}");
    }
    let too_few = "version = 1\n[profile]\nname = 'Bad EQ'\n\
        [[profile.filters]]\ntype = 'parametric_eq'\nbands = [{}, {}]";
    assert!(ProfileDocument::parse(too_few).is_err());
}

#[test]
fn crud_preserves_snapshots_and_loads_chain_and_limiter_without_devices() {
    let mut cfg = AppConfig {
        input_device: Some("Physical mic".into()),
        ..AppConfig::default()
    };
    cfg.limiter.enabled = true;
    let original = cfg.filters.clone();
    let first = cfg.save_new_profile(" Meetings ").unwrap();
    assert_eq!(cfg.profiles[first].name, "Meetings");
    cfg.filters.clear();
    cfg.limiter.ceiling_db = -6.0;
    assert_eq!(cfg.profiles[first].filters, original);
    assert!(!cfg.profiles[first].matches(&cfg));
    let copy = cfg.duplicate_profile(first, "Gaming").unwrap();
    assert_eq!(cfg.profiles[copy].filters, original);
    cfg.rename_profile(copy, "Streaming").unwrap();
    assert_eq!(cfg.profiles[copy].name, "Streaming");
    let mut next = 1;
    cfg.load_profile(first, &mut next).unwrap();
    assert!(cfg.profiles[first].matches(&cfg));
    assert_eq!(cfg.limiter.ceiling_db, -1.0);
    assert_eq!(cfg.input_device.as_deref(), Some("Physical mic"));
    cfg.filters.clear();
    cfg.overwrite_profile(copy).unwrap();
    assert!(cfg.profiles[copy].filters.is_empty());
    cfg.delete_profile(first).unwrap();
    assert_eq!(cfg.profiles.len(), 1);
    assert_eq!(cfg.profiles[0].name, "Streaming");
    assert!(cfg.filters.is_empty());
}

#[test]
fn names_are_trimmed_unique_and_validated_without_mutation_on_error() {
    let mut cfg = AppConfig::default();
    cfg.save_new_profile("Meetings").unwrap();
    let before = cfg.clone();
    for name in ["", "   ", "meetings", "MEETINGS", "bad\nname"] {
        assert!(cfg.save_new_profile(name).is_err());
    }
    assert!(cfg.save_new_profile(&"a".repeat(81)).is_err());
    assert!(cfg.duplicate_profile(0, "meetings").is_err());
    assert!(cfg.rename_profile(99, "Name").is_err());
    assert!(cfg.overwrite_profile(99).is_err());
    assert!(cfg.delete_profile(99).is_err());
    assert!(cfg.load_profile(99, &mut 1).is_err());
    assert_eq!(cfg, before);
}

#[test]
fn profile_load_assigns_fresh_nonzero_ids_including_wraparound() {
    let mut cfg = AppConfig::default();
    cfg.save_new_profile("Default").unwrap();
    let old: std::collections::HashSet<_> = cfg.filters.iter().map(|f| f.id).collect();
    let mut next_id = u64::MAX;
    cfg.load_profile(0, &mut next_id).unwrap();
    let new: std::collections::HashSet<_> = cfg.filters.iter().map(|f| f.id).collect();
    assert_eq!(new.len(), cfg.filters.len());
    assert!(!new.contains(&0));
    assert!(new.is_disjoint(&old));
    assert!(cfg.profiles[0].matches(&cfg));
}

#[test]
fn profiles_and_limiter_roundtrip_with_existing_config() {
    let mut cfg = AppConfig::default();
    cfg.limiter.enabled = true;
    cfg.limiter.ceiling_db = -3.0;
    cfg.save_new_profile("Meetings").unwrap();
    cfg.filters.clear();
    cfg.save_new_profile("Empty").unwrap();
    let text = toml::to_string_pretty(&cfg).unwrap();
    let back: AppConfig = toml::from_str(&text).unwrap();
    assert_eq!(back, cfg);
    let old: AppConfig = toml::from_str("filters = []").unwrap();
    assert!(!old.limiter.enabled);
    assert!(old.profiles.is_empty());
    assert!(old.filters.is_empty());
}

#[test]
fn portable_export_is_versioned_and_does_not_include_devices() {
    let mut cfg = AppConfig {
        input_device: Some("Private device name".into()),
        ..AppConfig::default()
    };
    cfg.save_new_profile("Meetings").unwrap();
    let text = ProfileDocument::export(&cfg.profiles[0]).unwrap();
    assert!(!text.contains("Private device name"));
    let profile = ProfileDocument::parse(&text).unwrap();
    assert_eq!(profile, cfg.profiles[0]);
    assert!(ProfileDocument::parse(&text.replace("version = 1", "version = 2")).is_err());
    assert!(cfg.add_profile(profile).is_err());
}

#[test]
fn import_rejects_bad_toml_nonfinite_out_of_range_and_oversized_documents() {
    assert!(ProfileDocument::parse("not TOML").is_err());
    assert!(ProfileDocument::parse(&" ".repeat(MAX_DOCUMENT_BYTES + 1)).is_err());
    for value in ["nan", "inf", "-inf", "10000.0"] {
        let document = format!(
            "version = 1\n[profile]\nname = 'Bad'\n[[profile.filters]]\ntype = 'gain'\ngain_db = {value}"
        );
        assert!(ProfileDocument::parse(&document).is_err());
    }
    let document = "version = 1\n[profile]\nname = 'Bad limiter'\nfilters = []\n[profile.limiter]\nceiling_db = nan";
    assert!(ProfileDocument::parse(document).is_err());
}

#[test]
fn import_normalizes_missing_and_duplicate_filter_ids() {
    let profile = ProfileDocument::parse(
        "version = 1\n[profile]\nname = 'Gains'\n\
         [[profile.filters]]\ntype = 'gain'\nid = 1\n\
         [[profile.filters]]\ntype = 'gain'\nid = 1\n\
         [[profile.filters]]\ntype = 'gain'\n",
    )
    .unwrap();
    let ids: std::collections::HashSet<_> = profile.filters.iter().map(|f| f.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&0));
}

#[test]
fn config_ignores_invalid_and_duplicate_saved_profiles_without_losing_chain() {
    let mut cfg = AppConfig::default();
    cfg.save_new_profile("Good").unwrap();
    cfg.profiles.push(cfg.profiles[0].clone());
    let mut invalid = cfg.profiles[0].clone();
    invalid.name = "Invalid".into();
    invalid.limiter.release_ms = -10.0;
    cfg.profiles.push(invalid);
    let back: AppConfig = toml::from_str(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
    assert_eq!(back.profiles.len(), 1);
    assert_eq!(back.profiles[0].name, "Good");
    assert_eq!(back.filters, cfg.filters);
}

#[test]
fn limits_prevent_unbounded_profile_counts_and_filter_chains() {
    let mut cfg = AppConfig {
        filters: vec![
            FilterConfig::new(1, FilterSettings::Gain(GainConfig::default()));
            MAX_FILTERS + 1
        ],
        ..AppConfig::default()
    };
    assert!(cfg.save_new_profile("Too many filters").is_err());
    cfg.filters.clear();
    for i in 0..MAX_PROFILES {
        cfg.save_new_profile(&format!("Profile {i}")).unwrap();
    }
    assert!(cfg.save_new_profile("One too many").is_err());
}
