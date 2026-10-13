//! Public preset documents preserve sound settings under explicit versioned keys.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
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
