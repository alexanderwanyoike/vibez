//! Verifies the complete canonical Delay factory sounds through native preset validation.

use super::*;
use std::collections::BTreeSet;
use vibez_dsp::delay::param;

const KEYS: [&str; param::COUNT] = [
    "time_ms",
    "feedback",
    "mix",
    "sync",
    "left_division",
    "left_modifier",
    "routing",
    "link",
    "right_time_ms",
    "right_division",
    "right_modifier",
    "feedback_hp_enabled",
    "feedback_hp_hz",
    "feedback_lp_enabled",
    "feedback_lp_hz",
    "duck",
    "time_change",
];

fn native_delay() -> UiEffect {
    let descriptors =
        vibez_dsp::factory::create_effect(EffectType::Delay, 48000.0).param_descriptors();
    UiEffect {
        reconfiguration_failed: false,
        latency_samples: Some(0),
        id: EffectId::new(),
        effect_type: EffectType::Delay,
        bypass: false,
        params: descriptors
            .iter()
            .map(|descriptor| descriptor.default)
            .collect(),
        descriptors,
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        sidechains: Vec::new(),
        inactive_sidechains: Vec::new(),
        external_inputs: Vec::new(),
    }
}

#[test]
fn factory_catalog_contains_eight_complete_canonical_delay_sounds() {
    let expected: [(&str, [f32; param::COUNT]); 8] = [
        (
            "Init",
            [
                500.0, 0.5, 0.3, 0.0, 4.0, 0.0, 0.0, 1.0, 500.0, 4.0, 0.0, 0.0, 120.0, 0.0, 8000.0,
                0.0, 0.0,
            ],
        ),
        (
            "Dotted Bounce",
            [
                500.0, 0.35, 0.22, 1.0, 3.0, 1.0, 0.0, 1.0, 500.0, 3.0, 1.0, 1.0, 120.0, 1.0,
                8000.0, 0.2, 0.0,
            ],
        ),
        (
            "Quarter Ping",
            [
                500.0, 0.45, 0.25, 1.0, 4.0, 0.0, 1.0, 1.0, 500.0, 4.0, 0.0, 1.0, 180.0, 1.0,
                7000.0, 0.35, 0.0,
            ],
        ),
        (
            "Short Slap",
            [
                90.0, 0.1, 0.18, 0.0, 4.0, 0.0, 0.0, 1.0, 90.0, 4.0, 0.0, 1.0, 120.0, 1.0, 10000.0,
                0.0, 0.0,
            ],
        ),
        (
            "Wide Eighths",
            [
                500.0, 0.32, 0.2, 1.0, 3.0, 0.0, 0.0, 0.0, 500.0, 3.0, 1.0, 1.0, 150.0, 1.0,
                9000.0, 0.1, 0.0,
            ],
        ),
        (
            "Triplet Taps",
            [
                500.0, 0.4, 0.24, 1.0, 3.0, 2.0, 1.0, 1.0, 500.0, 3.0, 2.0, 1.0, 160.0, 1.0,
                8000.0, 0.15, 0.0,
            ],
        ),
        (
            "Long Space",
            [
                500.0, 0.65, 0.18, 1.0, 6.0, 1.0, 0.0, 1.0, 500.0, 6.0, 1.0, 1.0, 250.0, 1.0,
                4000.0, 0.45, 0.0,
            ],
        ),
        (
            "Wet Return",
            [
                250.0, 0.25, 1.0, 0.0, 4.0, 0.0, 0.0, 1.0, 250.0, 4.0, 0.0, 1.0, 180.0, 1.0,
                6500.0, 0.0, 0.0,
            ],
        ),
    ];
    let presets = factory_presets(EffectType::Delay);
    assert_eq!(
        presets.iter().map(DevicePreset::name).collect::<Vec<_>>(),
        expected.iter().map(|(name, _)| *name).collect::<Vec<_>>()
    );
    assert_eq!(
        presets
            .iter()
            .map(DevicePreset::name)
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    let mut sounds = BTreeSet::new();
    let mut effect = native_delay();
    assert_eq!(effect.descriptors.len(), param::COUNT);
    let defaults: [u32; param::COUNT] =
        std::array::from_fn(|index| effect.descriptors[index].default.to_bits());
    assert_eq!(expected[0].1.map(f32::to_bits), defaults);
    let expected_keys: BTreeSet<_> = KEYS.into_iter().collect();
    for (preset, (name, values)) in presets.iter().zip(expected) {
        assert_eq!(preset.name(), name);
        assert_eq!(preset.kind(), DevicePresetKind::Effect(EffectType::Delay));
        assert_eq!(preset.parameters().len(), param::COUNT, "{name}");
        assert_eq!(
            preset
                .parameters()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected_keys,
            "{name}"
        );
        let actual = KEYS.map(|key| preset.parameters()[key]);
        assert_eq!(actual.map(f32::to_bits), values.map(f32::to_bits), "{name}");
        assert!(
            sounds.insert(actual.map(f32::to_bits)),
            "{name} duplicates another factory sound"
        );
        // An opposite valid boundary forces every key through the actual delta validator.
        effect.params = effect
            .descriptors
            .iter()
            .zip(values)
            .map(|(descriptor, value)| {
                if value != descriptor.min {
                    descriptor.min
                } else {
                    descriptor.max
                }
            })
            .collect();
        let updates =
            prepare_updates(&effect, preset).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(updates.len(), param::COUNT, "{name}");
        for (index, (actual_index, value)) in updates.into_iter().enumerate() {
            assert_eq!(actual_index, index, "{name}");
            assert_eq!(
                value.to_bits(),
                values[index].to_bits(),
                "{name} parameter{index}"
            );
            effect.params[index] = value;
        }
        let applied: [u32; param::COUNT] =
            std::array::from_fn(|index| effect.params[index].to_bits());
        assert_eq!(applied, values.map(f32::to_bits));
        assert!(
            prepare_updates(&effect, preset).unwrap().is_empty(),
            "{name} should become a no-op"
        );
    }
    assert_eq!(sounds.len(), 8);
}

#[test]
fn delay_presets_reject_fractional_choices_without_rejecting_any_valid_option() {
    let effect = native_delay();
    let initial = &factory_presets(EffectType::Delay)[0];
    let choices = [
        (param::SYNC, 1),
        (param::LEFT_DIVISION, 6),
        (param::LEFT_MODIFIER, 2),
        (param::ROUTING, 1),
        (param::LINK, 1),
        (param::RIGHT_DIVISION, 6),
        (param::RIGHT_MODIFIER, 2),
        (param::FEEDBACK_HP_ENABLED, 1),
        (param::FEEDBACK_LP_ENABLED, 1),
        (param::TIME_CHANGE, 1),
    ];
    let mut accepted_fractions = Vec::new();
    for (index, maximum) in choices {
        for ordinal in 0..=maximum {
            let mut parameters = initial.parameters().clone();
            parameters.insert(KEYS[index].into(), ordinal as f32);
            let preset =
                DevicePreset::new(initial.kind(), "Every valid option", parameters).unwrap();
            assert!(
                prepare_updates(&effect, &preset).is_ok(),
                "index{index} must accept ordinal{ordinal}"
            );
        }
        for value in [0.25, 0.5, 0.75, maximum as f32 - 0.5] {
            assert!(
                (effect.descriptors[index].min..=effect.descriptors[index].max).contains(&value)
            );
            let mut parameters = initial.parameters().clone();
            parameters.insert(KEYS[index].into(), value);
            let preset =
                DevicePreset::new(initial.kind(), "Fractional choice", parameters).unwrap();
            if prepare_updates(&effect, &preset).is_ok() {
                accepted_fractions.push((index, value));
            }
        }
    }
    let mut missing = initial.parameters().clone();
    missing.remove(KEYS[param::DUCK]);
    let mut extra = initial.parameters().clone();
    extra.insert("unknown_delay_parameter".into(), 0.0);
    for (kind, parameters) in [
        (initial.kind(), missing),
        (initial.kind(), extra),
        (
            DevicePresetKind::Effect(EffectType::Gain),
            initial.parameters().clone(),
        ),
    ] {
        let preset = DevicePreset::new(kind, "Wrong Delay shape or kind", parameters).unwrap();
        assert!(prepare_updates(&effect, &preset).is_err());
    }
    assert!(
        accepted_fractions.is_empty(),
        "Noncanonical preset choices were accepted: {accepted_fractions:?}"
    );
}
