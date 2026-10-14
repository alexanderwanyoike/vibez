//! Stable EQ preset keys and useful console starting points.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_dsp::eq::{param, EQ_PARAMS};

pub const KEYS: [&str; param::COUNT] = [
    "lf_gain_db",
    "lf_frequency_hz",
    "lf_bell",
    "lmf_gain_db",
    "lmf_frequency_hz",
    "lmf_q",
    "hmf_gain_db",
    "hmf_frequency_hz",
    "hmf_q",
    "hf_gain_db",
    "hf_frequency_hz",
    "hf_bell",
    "lf_enabled",
    "lmf_enabled",
    "hmf_enabled",
    "hf_enabled",
    "audition_band",
    "high_pass_enabled",
    "high_pass_hz",
    "low_pass_enabled",
    "low_pass_hz",
    "output_db",
];

pub const DISCRETE: [usize; 7] = [
    param::ENABLE[0],
    param::ENABLE[1],
    param::ENABLE[2],
    param::ENABLE[3],
    param::AUDITION,
    param::HP_ENABLED,
    param::LP_ENABLED,
];

pub fn factory_presets() -> Vec<DevicePreset> {
    let preset = |name: &str, changes: &[(usize, f32)]| {
        let mut values: Vec<_> = EQ_PARAMS
            .iter()
            .map(|descriptor| descriptor.default)
            .collect();
        for &(index, value) in changes {
            values[index] = value;
        }
        DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Eq),
            name,
            KEYS.iter()
                .zip(values)
                .map(|(key, value)| ((*key).into(), value))
                .collect(),
        )
        .expect("Factory EQ values are valid")
    };
    vec![
        preset("Flat", &[]),
        preset(
            "Gentle Presence",
            &[
                (param::GAIN[0], -1.0),
                (param::GAIN[2], 2.5),
                (param::FREQUENCY[2], 3200.0),
                (param::SHAPE[2], 0.8),
            ],
        ),
        preset(
            "Low Cleanup",
            &[
                (param::HP_ENABLED, 1.0),
                (param::HP_FREQUENCY, 70.0),
                (param::GAIN[1], -2.0),
                (param::FREQUENCY[1], 300.0),
                (param::SHAPE[1], 1.0),
            ],
        ),
        preset(
            "Warm Weight",
            &[
                (param::GAIN[0], 3.0),
                (param::FREQUENCY[0], 100.0),
                (param::GAIN[3], -2.0),
                (param::FREQUENCY[3], 7000.0),
            ],
        ),
        preset(
            "Vocal Focus",
            &[
                (param::HP_ENABLED, 1.0),
                (param::HP_FREQUENCY, 90.0),
                (param::GAIN[1], -3.0),
                (param::FREQUENCY[1], 350.0),
                (param::SHAPE[1], 1.0),
                (param::GAIN[2], 2.5),
                (param::FREQUENCY[2], 2800.0),
                (param::GAIN[3], 1.0),
                (param::FREQUENCY[3], 9000.0),
            ],
        ),
        preset(
            "Air Shelf",
            &[(param::GAIN[3], 4.0), (param::FREQUENCY[3], 12000.0)],
        ),
        preset(
            "Telephone",
            &[
                (param::HP_ENABLED, 1.0),
                (param::HP_FREQUENCY, 300.0),
                (param::LP_ENABLED, 1.0),
                (param::LP_FREQUENCY, 3400.0),
            ],
        ),
        preset(
            "Kick Shape",
            &[
                (param::GAIN[0], 4.0),
                (param::FREQUENCY[0], 65.0),
                (param::SHAPE[0], 1.0),
                (param::GAIN[1], -4.0),
                (param::FREQUENCY[1], 450.0),
                (param::SHAPE[1], 1.2),
                (param::GAIN[2], 2.0),
                (param::FREQUENCY[2], 3500.0),
            ],
        ),
        preset(
            "Find Mud",
            &[
                (param::FREQUENCY[1], 350.0),
                (param::SHAPE[1], 3.0),
                (param::AUDITION, 2.0),
                (param::OUTPUT, -3.0),
            ],
        ),
    ]
}
