//! Stable Drive sound keys and musical factory settings exclude channel identity and routing.

use vibez_core::{
    device_preset::{DevicePreset, DevicePresetKind},
    effect::EffectType,
};
use vibez_dsp::drive::{param, DRIVE_PARAMS};

pub const KEYS: [&str; param::COUNT] = [
    "amount",
    "tone",
    "output",
    "mix",
    "character",
    "quality",
    "pre_hp_enabled",
    "pre_hp_frequency_hz",
    "post_lp_enabled",
    "post_lp_frequency_hz",
];
pub const DISCRETE: [usize; 4] = [
    param::CHARACTER,
    param::QUALITY,
    param::PRE_HP_ENABLED,
    param::POST_LP_ENABLED,
];

pub fn factory_presets() -> Vec<DevicePreset> {
    use param::*;
    let gain = crate::domains::drive_values::amount_from_db;
    let amplitude = vibez_core::level::db_to_amplitude;
    let settings: [(&str, Vec<(usize, f32)>); 9] = [
        ("Init", vec![(AMOUNT, 0.0)]),
        ("Soft Init", vec![]),
        (
            "Warm Edge",
            vec![
                (AMOUNT, gain(6.0).unwrap()),
                (TONE, 0.55),
                (OUTPUT, amplitude(-3.0)),
                (MIX, 0.8),
            ],
        ),
        (
            "Drum Bite",
            vec![
                (CHARACTER, 2.0),
                (AMOUNT, gain(16.0).unwrap()),
                (TONE, 0.6),
                (OUTPUT, amplitude(-6.0)),
                (MIX, 0.7),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 40.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 16_000.0),
            ],
        ),
        (
            "Soft Bass",
            vec![
                (AMOUNT, gain(10.0).unwrap()),
                (TONE, 0.38),
                (OUTPUT, amplitude(-4.0)),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 25.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 8000.0),
            ],
        ),
        (
            "Bright Percussion",
            vec![
                (AMOUNT, gain(12.0).unwrap()),
                (TONE, 0.65),
                (OUTPUT, amplitude(-5.0)),
                (MIX, 0.6),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 120.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 18_000.0),
            ],
        ),
        (
            "Parallel Crunch",
            vec![
                (CHARACTER, 3.0),
                (AMOUNT, gain(24.0).unwrap()),
                (TONE, 0.55),
                (OUTPUT, amplitude(-8.0)),
                (MIX, 0.35),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 80.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 10_000.0),
            ],
        ),
        (
            "Controlled Hard",
            vec![
                (CHARACTER, 3.0),
                (AMOUNT, gain(18.0).unwrap()),
                (OUTPUT, amplitude(-6.0)),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 40.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 8500.0),
            ],
        ),
        (
            "Edge Lead",
            vec![
                (CHARACTER, 2.0),
                (AMOUNT, gain(14.0).unwrap()),
                (TONE, 0.6),
                (OUTPUT, amplitude(-5.0)),
                (MIX, 0.85),
                (PRE_HP_ENABLED, 1.0),
                (PRE_HP_FREQUENCY, 100.0),
                (POST_LP_ENABLED, 1.0),
                (POST_LP_FREQUENCY, 14_000.0),
            ],
        ),
    ];
    settings
        .into_iter()
        .map(|(name, edits)| {
            let mut values = if name == "Init" {
                DRIVE_PARAMS
                    .iter()
                    .map(|descriptor| descriptor.default)
                    .collect()
            } else {
                crate::domains::drive::fresh_sound()
            };
            for (index, value) in edits {
                values[index] = value;
            }
            DevicePreset::new(
                DevicePresetKind::Effect(EffectType::Drive),
                name,
                KEYS.iter()
                    .zip(values)
                    .map(|(key, value)| ((*key).into(), value))
                    .collect(),
            )
            .expect("Factory Drive values are valid")
        })
        .collect()
}
