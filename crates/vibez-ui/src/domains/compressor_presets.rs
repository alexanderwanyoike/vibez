//! Stable Compressor sound keys deliberately exclude channel and detector routing.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_dsp::compressor::{param, COMPRESSOR_PARAMS};

pub const KEYS: [&str; param::COUNT] = [
    "threshold_db",
    "ratio",
    "attack_ms",
    "release_ms",
    "makeup_db",
    "mix",
    "knee_db",
    "hp_enabled",
    "hp_frequency_hz",
    "lp_enabled",
    "lp_frequency_hz",
    "detector_mode",
    "auto_release",
    "stereo_link",
    "detector_listen",
    "range_enabled",
    "range_db",
];

pub const DISCRETE: [usize; 7] = [
    param::HP_ENABLED,
    param::LP_ENABLED,
    param::DETECTOR_MODE,
    param::AUTO_RELEASE,
    param::STEREO_LINK,
    param::DETECTOR_LISTEN,
    param::RANGE_ENABLED,
];

pub fn factory_presets() -> Vec<DevicePreset> {
    use param::*;
    let settings: &[(&str, &[(usize, f32)])] = &[
        ("Init", &[]),
        (
            "Clean Bus",
            &[
                (THRESHOLD, -16.0),
                (RATIO, 2.0),
                (ATTACK, 20.0),
                (RELEASE, 150.0),
                (MAKEUP, 1.0),
                (KNEE, 6.0),
                (HP_ENABLED, 1.0),
            ],
        ),
        (
            "Drum Punch",
            &[
                (THRESHOLD, -18.0),
                (RATIO, 4.0),
                (ATTACK, 25.0),
                (RELEASE, 80.0),
                (KNEE, 3.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 60.0),
            ],
        ),
        (
            "Vocal Level",
            &[
                (THRESHOLD, -24.0),
                (RATIO, 3.0),
                (ATTACK, 4.0),
                (RELEASE, 120.0),
                (MAKEUP, 5.0),
                (KNEE, 9.0),
                (DETECTOR_MODE, 1.0),
                (AUTO_RELEASE, 1.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 100.0),
            ],
        ),
        (
            "Gentle Glue",
            &[
                (THRESHOLD, -16.0),
                (RATIO, 1.8),
                (ATTACK, 30.0),
                (RELEASE, 200.0),
                (MAKEUP, 1.0),
                (KNEE, 12.0),
                (AUTO_RELEASE, 1.0),
                (HP_ENABLED, 1.0),
                (HP_FREQUENCY, 90.0),
            ],
        ),
        (
            "Parallel Crush",
            &[
                (THRESHOLD, -30.0),
                (RATIO, 10.0),
                (ATTACK, 15.0),
                (RELEASE, 140.0),
                (MAKEUP, 12.0),
                (MIX, 0.25),
                (KNEE, 3.0),
                (RANGE_ENABLED, 1.0),
                (RANGE, 24.0),
            ],
        ),
        (
            "Kick Duck",
            &[
                (THRESHOLD, -24.0),
                (RATIO, 8.0),
                (ATTACK, 2.0),
                (RELEASE, 140.0),
                (MAKEUP, 0.0),
                (KNEE, 3.0),
                (LP_ENABLED, 1.0),
                (LP_FREQUENCY, 1800.0),
            ],
        ),
        (
            "Bass Control",
            &[
                (THRESHOLD, -22.0),
                (RATIO, 4.0),
                (ATTACK, 10.0),
                (RELEASE, 150.0),
                (DETECTOR_MODE, 1.0),
                (LP_ENABLED, 1.0),
                (LP_FREQUENCY, 5000.0),
            ],
        ),
        (
            "Soft RMS",
            &[
                (THRESHOLD, -20.0),
                (RATIO, 2.5),
                (ATTACK, 15.0),
                (RELEASE, 160.0),
                (MAKEUP, 2.0),
                (KNEE, 12.0),
                (DETECTOR_MODE, 1.0),
                (AUTO_RELEASE, 1.0),
            ],
        ),
    ];
    settings
        .iter()
        .map(|(name, edits)| {
            let mut values = COMPRESSOR_PARAMS
                .iter()
                .map(|descriptor| descriptor.default)
                .collect::<Vec<_>>();
            for &(index, value) in *edits {
                values[index] = value;
            }
            DevicePreset::new(
                DevicePresetKind::Effect(EffectType::Compressor),
                *name,
                KEYS.iter()
                    .zip(values)
                    .map(|(key, value)| ((*key).into(), value))
                    .collect(),
            )
            .expect("Factory Compressor values are valid")
        })
        .collect()
}
