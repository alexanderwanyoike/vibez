//! Musical Bitcrush sound assets retain stable stored dividers independently of host rate.

use vibez_core::{
    device_preset::{DevicePreset, DevicePresetKind},
    effect::EffectType,
};
use vibez_dsp::bitcrush::param;

pub const DISCRETE: [usize; 3] = [param::ROUNDING, param::DITHER, param::POST_LP_ENABLED];

pub const KEYS: [&str; 9] = [
    "bits",
    "sr_reduce",
    "mix",
    "drive_db",
    "rounding",
    "dither",
    "jitter",
    "post_lp_enabled",
    "post_lp_frequency_hz",
];

pub fn fresh_sound() -> [f32; 9] {
    [12.0, 4.0, 0.65, 2.0, 0.0, 0.0, 0.0, 0.0, 12_000.0]
}

pub fn factory_presets() -> Vec<DevicePreset> {
    let sounds = [
        ("Init", [16.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0]),
        ("Crunchy 12", fresh_sound()),
        ("Drum Crunch", [8.0, 3.0, 0.55, 6.0, 0.0, 0.0, 0.0, 0.0, 12_000.0]),
        ("Dusty Bass", [10.0, 6.0, 0.7, 3.0, 0.0, 0.0, 0.0, 1.0, 3500.0]),
        ("Pocket Sampler", [8.0, 4.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 12_000.0]),
        ("Broken Clock", [12.0, 8.0, 0.75, 2.0, 0.0, 0.0, 0.35, 1.0, 6000.0]),
        ("Parallel Arcade", [4.0, 10.0, 0.3, 8.0, 2.0, 0.0, 0.0, 0.0, 12_000.0]),
        ("Digital Air", [6.0, 1.0, 0.2, 0.0, 0.0, 1.0, 0.0, 0.0, 12_000.0]),
    ];
    sounds
        .into_iter()
        .map(|(name, values)| {
            DevicePreset::new(
                DevicePresetKind::Effect(EffectType::Bitcrush),
                name,
                KEYS.iter()
                    .zip(values)
                    .map(|(key, value)| ((*key).into(), value))
                    .collect(),
            )
            .expect("Factory Bitcrush values are valid")
        })
        .collect()
}
