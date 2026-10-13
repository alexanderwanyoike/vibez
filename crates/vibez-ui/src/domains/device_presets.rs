//! Converts portable presets into validated edits of existing built-in devices.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;

use crate::state::UiEffect;

const GAIN_KEYS: [&str; 5] = [
    "gain",
    "left_trim_db",
    "right_trim_db",
    "left_polarity",
    "right_polarity",
];

fn parameter_keys(effect: &UiEffect) -> Result<&'static [&'static str], String> {
    if effect.plugin_ref.is_some() || effect.plugin_name.is_some() {
        return Err("Use the plugin's own preset controls for this device".into());
    }
    match effect.effect_type {
        EffectType::Gain => Ok(&GAIN_KEYS),
        _ => Err("Presets are not yet supported by this built-in device".into()),
    }
}

pub fn capture(effect: &UiEffect, name: &str) -> Result<DevicePreset, String> {
    let keys = parameter_keys(effect)?;
    let parameters = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let descriptor = effect.descriptors.get(index)
                .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
            let value = effect.params.get(index).copied().unwrap_or(descriptor.default);
            Ok(((*key).to_owned(), value))
        })
        .collect::<Result<_, String>>()?;
    DevicePreset::new(DevicePresetKind::Effect(effect.effect_type), name, parameters)
        .map_err(|error| error.to_string())
}

pub fn prepare_updates(effect: &UiEffect, preset: &DevicePreset) -> Result<Vec<(usize, f32)>, String> {
    let keys = parameter_keys(effect)?;
    if preset.kind() != DevicePresetKind::Effect(effect.effect_type) {
        return Err("This preset belongs to a different device".into());
    }
    if preset.parameters().len() != keys.len() {
        return Err("The preset does not contain this device's complete parameter set".into());
    }
    let mut updates = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let value = *preset.parameters().get(*key)
            .ok_or_else(|| format!("The preset is missing parameter {key}"))?;
        let descriptor = effect.descriptors.get(index)
            .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
        if !value.is_finite() || !(descriptor.min..=descriptor.max).contains(&value)
            || (index >= 3 && value != 0.0 && value != 1.0)
        {
            return Err(format!("Preset parameter {key} is outside its supported range"));
        }
        if effect.params.get(index).copied().unwrap_or(descriptor.default) != value {
            updates.push((index, value));
        }
    }
    Ok(updates)
}

pub fn factory_presets(effect_type: EffectType) -> Vec<DevicePreset> {
    if effect_type != EffectType::Gain {
        return Vec::new();
    }
    [
        ("Unity", [1.0, 0.0, 0.0, 0.0, 0.0]),
        ("Minus 6 dB", [10.0_f32.powf(-6.0 / 20.0), 0.0, 0.0, 0.0, 0.0]),
        ("Minus 12 dB", [10.0_f32.powf(-12.0 / 20.0), 0.0, 0.0, 0.0, 0.0]),
        ("Invert stereo", [1.0, 0.0, 0.0, 1.0, 1.0]),
    ]
    .into_iter()
    .map(|(name, values)| {
        DevicePreset::new(
            DevicePresetKind::Effect(effect_type),
            name,
            GAIN_KEYS.iter().zip(values).map(|(key, value)| ((*key).into(), value)).collect(),
        ).expect("Factory preset values are valid")
    })
    .collect()
}
