//! Stable Gate sound keys and factory settings exclude channel and detector routing.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_dsp::gate::{param, GATE_PARAMS};

pub const KEYS: [&str; param::COUNT] = [
    "threshold_db",
    "attack_ms",
    "release_ms",
    "hold_ms",
    "hysteresis_db",
    "range_enabled",
    "range_db",
    "mode",
    "expansion_ratio",
    "hp_enabled",
    "hp_frequency_hz",
    "lp_enabled",
    "lp_frequency_hz",
    "detector_listen",
    "mix",
];
pub const DISCRETE: [usize; 5] = [
    param::RANGE_ENABLED,
    param::MODE,
    param::HP_ENABLED,
    param::LP_ENABLED,
    param::DETECTOR_LISTEN,
];

pub fn factory_presets() -> Vec<DevicePreset> {
    use param::*;
    let settings: &[(&str, &[(usize, f32)])] = &[
        ("Init", &[]),
        (
            "Tight Drums",
            &[
                (THRESHOLD, -32.0),
                (ATTACK, 1.0),
                (HOLD, 60.0),
                (RELEASE, 120.0),
                (HYSTERESIS, 6.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 60.0),
            ],
        ),
        (
            "Gentle Cleanup",
            &[
                (THRESHOLD, -48.0),
                (ATTACK, 3.0),
                (HOLD, 30.0),
                (RELEASE, 180.0),
                (HYSTERESIS, 4.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 18.0),
            ],
        ),
        (
            "Snare Body",
            &[
                (THRESHOLD, -28.0),
                (ATTACK, 0.5),
                (HOLD, 85.0),
                (RELEASE, 170.0),
                (HYSTERESIS, 5.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 160.0),
                (LP_ENABLED, 1.0),
                (LP_FREQUENCY, 6000.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 48.0),
            ],
        ),
        (
            "Short Percussion",
            &[
                (THRESHOLD, -34.0),
                (ATTACK, 0.2),
                (HOLD, 12.0),
                (RELEASE, 35.0),
                (HYSTERESIS, 3.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 72.0),
            ],
        ),
        (
            "Vocal Expander",
            &[
                (THRESHOLD, -42.0),
                (ATTACK, 4.0),
                (HOLD, 35.0),
                (RELEASE, 210.0),
                (HYSTERESIS, 2.0),
                (MODE, 1.0),
                (EXPANSION_RATIO, 2.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 15.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 100.0),
            ],
        ),
        (
            "External Trigger",
            &[
                (THRESHOLD, -24.0),
                (ATTACK, 0.2),
                (HOLD, 45.0),
                (RELEASE, 90.0),
                (HYSTERESIS, 4.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 40.0),
                (LP_ENABLED, 1.0),
                (LP_FREQUENCY, 1800.0),
            ],
        ),
        (
            "Soft Floor",
            &[
                (THRESHOLD, -38.0),
                (ATTACK, 8.0),
                (HOLD, 20.0),
                (RELEASE, 250.0),
                (HYSTERESIS, 5.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 12.0),
            ],
        ),
    ];
    settings
        .iter()
        .map(|(name, edits)| {
            let mut values = GATE_PARAMS
                .iter()
                .map(|descriptor| descriptor.default)
                .collect::<Vec<_>>();
            for &(index, value) in *edits {
                values[index] = value;
            }
            DevicePreset::new(
                DevicePresetKind::Effect(EffectType::Gate),
                *name,
                KEYS.iter()
                    .zip(values)
                    .map(|(key, value)| ((*key).into(), value))
                    .collect(),
            )
            .expect("Factory Gate values are valid")
        })
        .collect()
}
