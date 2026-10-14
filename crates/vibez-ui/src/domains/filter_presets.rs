//! Stable Filter preset keys and musical factory starting points.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_dsp::filter::param;

pub const KEYS: [&str; param::COUNT] = [
    "cutoff_hz",
    "resonance_q",
    "mode",
    "slope",
    "drive_db",
    "mix",
    "mod_source",
    "rate_hz",
    "depth_octaves",
    "sync",
    "stereo_phase_degrees",
    "envelope_attack_ms",
    "envelope_release_ms",
];
pub const DISCRETE: [usize; 4] = [param::MODE, param::SLOPE, param::MOD_SOURCE, param::SYNC];

pub fn factory_presets() -> Vec<DevicePreset> {
    [
        (
            "Gentle Sweep",
            [
                1200.0, 0.707, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
            ],
        ),
        (
            "Subsonic Cut",
            [
                80.0, 0.707, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
            ],
        ),
        (
            "Telephone",
            [
                1000.0, 0.5, 2.0, 0.0, 2.0, 0.8, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
            ],
        ),
        (
            "Resonant Edge",
            [
                2200.0, 3.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
            ],
        ),
        (
            "Hum Notch",
            [
                50.0, 5.0, 3.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
            ],
        ),
        (
            "Envelope Wah",
            [
                400.0, 1.0, 0.0, 1.0, 0.0, 1.0, 2.0, 1.0, 3.0, 0.0, 0.0, 10.0, 200.0,
            ],
        ),
        (
            "Stereo Eighths",
            [
                1800.0, 0.9, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 4.0, 90.0, 10.0, 150.0,
            ],
        ),
    ]
    .into_iter()
    .map(|(name, values)| {
        DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Filter),
            name,
            KEYS.iter()
                .zip(values)
                .map(|(key, value)| ((*key).into(), value))
                .collect(),
        )
        .expect("Factory Filter values are valid")
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_presets::{capture, prepare_updates};
    use crate::state::UiEffect;
    fn effect() -> UiEffect {
        UiEffect {
            id: vibez_core::id::EffectId::new(),
            effect_type: EffectType::Filter,
            params: vibez_dsp::filter::FILTER_PARAMS
                .iter()
                .map(|p| p.default)
                .collect(),
            descriptors: vibez_dsp::filter::FILTER_PARAMS,
            bypass: false,
            plugin_name: None,
            plugin_ref: None,
            has_plugin_gui: false,
            latency_samples: Some(0),
            reconfiguration_failed: false,
            sidechains: vec![],
            inactive_sidechains: vec![],
            external_inputs: vec![],
        }
    }
    #[test]
    fn all_factories_round_trip_and_become_noop_after_application() {
        let presets = factory_presets();
        assert!(presets.len() >= 6);
        for preset in presets {
            let mut effect = effect();
            let serialized = serde_json::to_vec(&preset).unwrap();
            let restored = serde_json::from_slice(&serialized).unwrap();
            assert_eq!(preset, restored);
            for (index, value) in prepare_updates(&effect, &restored).unwrap() {
                effect.params[index] = value;
            }
            assert!(prepare_updates(&effect, &restored).unwrap().is_empty());
            assert_eq!(capture(&effect, restored.name()).unwrap(), restored);
        }
    }
    #[test]
    fn fractional_selectors_and_incomplete_presets_are_rejected() {
        let effect = effect();
        let preset = capture(&effect, "Current").unwrap();
        for index in DISCRETE {
            let mut values = preset.parameters().clone();
            values.insert(KEYS[index].into(), 0.5);
            let malformed = DevicePreset::new(preset.kind(), "Fractional", values).unwrap();
            assert!(prepare_updates(&effect, &malformed).is_err());
        }
        let mut values = preset.parameters().clone();
        values.remove(KEYS[param::MIX]);
        assert!(prepare_updates(
            &effect,
            &DevicePreset::new(preset.kind(), "Incomplete", values).unwrap()
        )
        .is_err());
    }
}
