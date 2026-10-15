//! Defines complete canonical Delay factory sounds and their stable parameter keys.

use super::{DevicePreset, DevicePresetKind, EffectType};
use vibez_dsp::delay::param;

pub(super) const KEYS: [&str; param::COUNT] = [
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

pub(super) fn factory_presets() -> Vec<DevicePreset> {
    let sounds: [(&str, [f32; param::COUNT]); 8] = [
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
    sounds
        .into_iter()
        .map(|(name, values)| {
            DevicePreset::new(
                DevicePresetKind::Effect(EffectType::Delay),
                name,
                KEYS.into_iter()
                    .zip(values)
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect(),
            )
            .expect("Delay factory preset values are valid")
        })
        .collect()
}

pub(super) fn is_canonical(index: usize, value: f32) -> bool {
    !matches!(
        index,
        param::SYNC
            | param::LEFT_DIVISION
            | param::LEFT_MODIFIER
            | param::ROUTING
            | param::LINK
            | param::RIGHT_DIVISION
            | param::RIGHT_MODIFIER
            | param::FEEDBACK_HP_ENABLED
            | param::FEEDBACK_LP_ENABLED
            | param::TIME_CHANGE
    ) || value.fract() == 0.0
}
