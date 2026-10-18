//! Public preset documents preserve sound settings under explicit versioned keys.

use vibez_core::device_preset::{DevicePreset, DevicePresetError, DevicePresetKind};
use vibez_core::effect::EffectType;

#[test]
fn versioned_preset_round_trips_stable_parameters_and_rejects_unknown_versions() {
    let document = serde_json::json!({
        "format_version": 1,
        "kind": { "effect": "Gain" },
        "name": "Quiet reference",
        "parameters": { "gain": 0.5 }
    });

    let preset: DevicePreset = serde_json::from_value(document.clone()).unwrap();

    assert_eq!(preset.kind(), DevicePresetKind::Effect(EffectType::Gain));
    assert_eq!(preset.name(), "Quiet reference");
    assert_eq!(preset.parameters().get("gain"), Some(&0.5));
    assert_eq!(serde_json::to_value(&preset).unwrap(), document);

    let mut unsupported = document;
    unsupported["format_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<DevicePreset>(unsupported).is_err());
}

#[test]
fn constructing_a_preset_rejects_nonfinite_sound_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let result = DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Gain),
            "Reference",
            [("gain".to_string(), value)].into_iter().collect(),
        );

        assert_eq!(
            result,
            Err(DevicePresetError::NonFiniteParameter("gain".to_string()))
        );
    }
}

#[test]
fn decoding_a_preset_rejects_values_outside_finite_f32_range() {
    let document = serde_json::json!({
        "format_version": 1,
        "kind": { "effect": "Gain" },
        "name": "Reference",
        "parameters": { "gain": 1.0e100 }
    });

    assert!(serde_json::from_value::<DevicePreset>(document).is_err());
}

#[test]
fn preset_names_must_be_readable_and_bounded_for_construction_and_decoding() {
    for name in [
        "".to_string(),
        "   ".to_string(),
        "two\nlines".to_string(),
        "x".repeat(129),
    ] {
        let result = DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Gain),
            &name,
            [("gain".to_string(), 1.0)].into_iter().collect(),
        );
        assert_eq!(result, Err(DevicePresetError::InvalidName));

        let document = serde_json::json!({
            "format_version": 1,
            "kind": { "effect": "Gain" },
            "name": name,
            "parameters": { "gain": 1.0 }
        });
        assert!(serde_json::from_value::<DevicePreset>(document).is_err());
    }
}

#[test]
fn parameter_keys_must_be_stable_and_bounded_for_construction_and_decoding() {
    for key in ["".to_string(), "gain value".to_string(), "x".repeat(129)] {
        let result = DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Gain),
            "Reference",
            [(key.clone(), 1.0)].into_iter().collect(),
        );
        assert_eq!(
            result,
            Err(DevicePresetError::InvalidParameterKey(key.clone()))
        );

        let document = serde_json::json!({
            "format_version": 1,
            "kind": { "effect": "Gain" },
            "name": "Reference",
            "parameters": { key: 1.0 }
        });
        assert!(serde_json::from_value::<DevicePreset>(document).is_err());
    }
}
