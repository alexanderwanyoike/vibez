//! Built-in device preset selection and validated parameter changes.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_core::id::{EffectId, TrackId};

use crate::state::UiEffect;

#[derive(Debug, Clone)]
pub enum DevicePresetMsg {
    Load(TrackId, EffectId),
    Save(TrackId, EffectId),
    Loaded(PresetLoadRequest, Result<Option<DevicePreset>, String>),
    Saved(PresetSaveRequest, Result<Option<DevicePreset>, String>),
}

#[derive(Debug, Clone)]
pub struct PresetSaveRequest {
    pub token: u64,
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub preset: DevicePreset,
}

#[derive(Debug, Clone)]
pub struct PresetLoadRequest {
    pub token: u64,
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub expected: DevicePreset,
}

const GAIN_KEYS: [&str; 5] = [
    "gain",
    "left_trim_db",
    "right_trim_db",
    "left_polarity",
    "right_polarity",
];

pub struct PresetPresentation {
    pub options: Vec<DevicePreset>,
    pub selected: Option<DevicePreset>,
    pub modified: bool,
    pub previous: Option<DevicePreset>,
    pub next: Option<DevicePreset>,
}

pub fn presentation(effect: &UiEffect, remembered: Option<&DevicePreset>) -> PresetPresentation {
    let presets = factory_presets(effect.effect_type);
    let selected = remembered.cloned().or_else(|| {
        presets
            .iter()
            .find(|preset| prepare_updates(effect, preset).is_ok_and(|updates| updates.is_empty()))
            .cloned()
    });
    let position = selected
        .as_ref()
        .and_then(|selected| presets.iter().position(|preset| preset == selected));
    let adjacent = |forward: bool| {
        if presets.is_empty() {
            return None;
        }
        let index = match (position, forward) {
            (Some(index), true) => (index + 1) % presets.len(),
            (Some(index), false) => (index + presets.len() - 1) % presets.len(),
            (None, true) => 0,
            (None, false) => presets.len() - 1,
        };
        Some(presets[index].clone())
    };
    let previous = adjacent(false);
    let next = adjacent(true);
    let modified = selected.as_ref().is_some_and(|preset| {
        prepare_updates(effect, preset).map_or(true, |updates| !updates.is_empty())
    });
    PresetPresentation {
        options: presets,
        selected,
        modified,
        previous,
        next,
    }
}

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
            let descriptor = effect
                .descriptors
                .get(index)
                .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
            let value = effect
                .params
                .get(index)
                .copied()
                .unwrap_or(descriptor.default);
            Ok(((*key).to_owned(), value))
        })
        .collect::<Result<_, String>>()?;
    DevicePreset::new(
        DevicePresetKind::Effect(effect.effect_type),
        name,
        parameters,
    )
    .map_err(|error| error.to_string())
}

pub fn prepare_updates(
    effect: &UiEffect,
    preset: &DevicePreset,
) -> Result<Vec<(usize, f32)>, String> {
    let keys = parameter_keys(effect)?;
    if preset.kind() != DevicePresetKind::Effect(effect.effect_type) {
        return Err("This preset belongs to a different device".into());
    }
    if preset.parameters().len() != keys.len() {
        return Err("The preset does not contain this device's complete parameter set".into());
    }
    let mut updates = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let value = *preset
            .parameters()
            .get(*key)
            .ok_or_else(|| format!("The preset is missing parameter {key}"))?;
        let descriptor = effect
            .descriptors
            .get(index)
            .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
        if !value.is_finite()
            || !(descriptor.min..=descriptor.max).contains(&value)
            || (effect.effect_type == EffectType::Gain
                && vibez_dsp::gain::param::POLARITY.contains(&index)
                && value != 0.0
                && value != 1.0)
        {
            return Err(format!(
                "Preset parameter {key} is outside its supported range"
            ));
        }
        if effect
            .params
            .get(index)
            .copied()
            .unwrap_or(descriptor.default)
            != value
        {
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
        (
            "Minus 6 dB",
            [vibez_core::level::db_to_amplitude(-6.0), 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "Minus 12 dB",
            [
                vibez_core::level::db_to_amplitude(-12.0),
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        ),
        (
            "Boost 6 dB",
            [vibez_core::level::db_to_amplitude(6.0), 0.0, 0.0, 0.0, 0.0],
        ),
        ("Balance left 3 dB", [1.0, 0.0, -3.0, 0.0, 0.0]),
        ("Balance right 3 dB", [1.0, -3.0, 0.0, 0.0, 0.0]),
        ("Invert left", [1.0, 0.0, 0.0, 1.0, 0.0]),
        ("Invert right", [1.0, 0.0, 0.0, 0.0, 1.0]),
        ("Invert stereo", [1.0, 0.0, 0.0, 1.0, 1.0]),
    ]
    .into_iter()
    .map(|(name, values)| {
        DevicePreset::new(
            DevicePresetKind::Effect(effect_type),
            name,
            GAIN_KEYS
                .iter()
                .zip(values)
                .map(|(key, value)| ((*key).into(), value))
                .collect(),
        )
        .expect("Factory preset values are valid")
    })
    .collect()
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use vibez_core::id::EffectId;

    #[test]
    fn gain_factory_presets_cover_balance_and_single_channel_polarity() {
        let presets = factory_presets(EffectType::Gain);
        let values = |name: &str| {
            let preset = presets
                .iter()
                .find(|preset| preset.name() == name)
                .unwrap_or_else(|| panic!("missing {name}"));
            GAIN_KEYS.map(|key| preset.parameters()[key])
        };
        assert_eq!(values("Balance left 3 dB"), [1.0, 0.0, -3.0, 0.0, 0.0]);
        assert_eq!(values("Balance right 3 dB"), [1.0, -3.0, 0.0, 0.0, 0.0]);
        assert_eq!(values("Invert left"), [1.0, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(values("Invert right"), [1.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(values("Boost 6 dB")[0] > 1.99);
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    fn gain() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0).param_descriptors();
        UiEffect {
            reconfiguration_failed: false,
            latency_samples: Some(0),
            id: EffectId::new(),
            effect_type: EffectType::Gain,
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
    fn matching_factory_selection_cycles_in_both_directions() {
        let mut effect = gain();
        let initial = presentation(&effect, None);
        assert_eq!(initial.selected, Some(initial.options[0].clone()));
        assert!(!initial.modified);
        assert_eq!(initial.previous, initial.options.last().cloned());
        assert_eq!(initial.next, Some(initial.options[1].clone()));

        let last = initial.previous.unwrap();
        for (index, value) in prepare_updates(&effect, &last).unwrap() {
            effect.params[index] = value;
        }
        let inverted = presentation(&effect, None);
        assert_eq!(inverted.selected, Some(last));
        assert!(!inverted.modified);
        assert_eq!(inverted.next, Some(initial.options[0].clone()));
        assert_eq!(
            inverted.previous,
            Some(initial.options[initial.options.len() - 2].clone())
        );
    }

    #[test]
    fn edited_factory_and_user_presets_keep_their_selection_and_modified_state() {
        let mut effect = gain();
        let unity = factory_presets(EffectType::Gain)[0].clone();
        effect.params[0] = 0.25;
        let edited = presentation(&effect, Some(&unity));
        assert_eq!(edited.selected, Some(unity));
        assert!(edited.modified);
        assert_eq!(edited.next, Some(edited.options[1].clone()));

        let user = capture(&effect, "Quiet bass").unwrap();
        let unchanged = presentation(&effect, Some(&user));
        assert_eq!(unchanged.selected, Some(user));
        assert!(!unchanged.modified);
        assert_eq!(unchanged.previous, unchanged.options.last().cloned());
        assert_eq!(unchanged.next, unchanged.options.first().cloned());

        let custom = presentation(&effect, None);
        assert!(custom.selected.is_none());
        assert!(!custom.modified);
        assert_eq!(custom.previous, custom.options.last().cloned());
        assert_eq!(custom.next, custom.options.first().cloned());
    }

    #[test]
    fn a_device_without_factory_presets_has_no_navigation() {
        let mut effect = gain();
        effect.effect_type = EffectType::Reverb;
        let unsupported = presentation(&effect, None);
        assert!(unsupported.options.is_empty());
        assert!(unsupported.selected.is_none());
        assert!(unsupported.previous.is_none());
        assert!(unsupported.next.is_none());
    }
}
