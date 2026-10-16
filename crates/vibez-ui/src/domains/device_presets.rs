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

/// Native EQ parameters a preset stores: everything but the soloed band,
/// which is a listening aid rather than part of the sound. Switches must be
/// exactly on or off.
const EQ_KEYS: [(usize, &str, bool); 21] = {
    use vibez_dsp::eq::param::*;
    [
        (GAIN[0], "lf_gain_db", false),
        (FREQUENCY[0], "lf_frequency_hz", false),
        (SHAPE[0], "lf_bell", true),
        (GAIN[1], "lmf_gain_db", false),
        (FREQUENCY[1], "lmf_frequency_hz", false),
        (SHAPE[1], "lmf_q", false),
        (GAIN[2], "hmf_gain_db", false),
        (FREQUENCY[2], "hmf_frequency_hz", false),
        (SHAPE[2], "hmf_q", false),
        (GAIN[3], "hf_gain_db", false),
        (FREQUENCY[3], "hf_frequency_hz", false),
        (SHAPE[3], "hf_bell", true),
        (ENABLE[0], "lf_enabled", true),
        (ENABLE[1], "lmf_enabled", true),
        (ENABLE[2], "hmf_enabled", true),
        (ENABLE[3], "hf_enabled", true),
        (HP_ENABLED, "low_cut_enabled", true),
        (HP_FREQUENCY, "low_cut_hz", false),
        (LP_ENABLED, "high_cut_enabled", true),
        (LP_FREQUENCY, "high_cut_hz", false),
        (OUTPUT, "output_db", false),
    ]
};

/// Native Compressor parameters a preset stores: everything but detector
/// listening, which is a monitoring aid rather than part of the sound.
const COMPRESSOR_KEYS: [(usize, &str, bool); 16] = {
    use vibez_dsp::compressor::param::*;
    [
        (THRESHOLD, "threshold_db", false),
        (RATIO, "ratio", false),
        (ATTACK, "attack_ms", false),
        (RELEASE, "release_ms", false),
        (MAKEUP, "makeup_db", false),
        (MIX, "mix", false),
        (KNEE, "knee_db", false),
        (HP_ENABLED, "hp_enabled", true),
        (HP_FREQUENCY, "hp_frequency_hz", false),
        (LP_ENABLED, "lp_enabled", true),
        (LP_FREQUENCY, "lp_frequency_hz", false),
        (DETECTOR_MODE, "detector_mode", true),
        (AUTO_RELEASE, "auto_release", true),
        (STEREO_LINK, "stereo_link", true),
        (RANGE_ENABLED, "range_enabled", true),
        (RANGE, "range_db", false),
    ]
};

/// Every native Filter parameter; its type, slope, source and sync are
/// choices.
const FILTER_KEYS: [(usize, &str, bool); 13] = {
    use vibez_dsp::filter::param::*;
    [
        (CUTOFF, "cutoff_hz", false),
        (RESONANCE, "resonance_q", false),
        (MODE, "mode", true),
        (SLOPE, "slope", true),
        (DRIVE, "drive_db", false),
        (MIX, "mix", false),
        (MOD_SOURCE, "mod_source", true),
        (RATE, "rate_hz", false),
        (DEPTH, "depth_octaves", false),
        (SYNC, "sync", true),
        (STEREO_PHASE, "stereo_phase_degrees", false),
        (ENV_ATTACK, "envelope_attack_ms", false),
        (ENV_RELEASE, "envelope_release_ms", false),
    ]
};

/// Native Gate parameters a preset stores: everything but detector
/// audition, which is a listening aid rather than part of the sound.
const GATE_KEYS: [(usize, &str, bool); 14] = {
    use vibez_dsp::gate::param::*;
    [
        (THRESHOLD, "threshold_db", false),
        (ATTACK, "attack_ms", false),
        (RELEASE, "release_ms", false),
        (HOLD, "hold_ms", false),
        (HYSTERESIS, "hysteresis_db", false),
        (RANGE_ENABLED, "range_enabled", true),
        (RANGE, "range_db", false),
        (MODE, "mode", true),
        (EXPANSION_RATIO, "expansion_ratio", false),
        (HP_ENABLED, "hp_enabled", true),
        (HP_FREQUENCY, "hp_frequency_hz", false),
        (LP_ENABLED, "lp_enabled", true),
        (LP_FREQUENCY, "lp_frequency_hz", false),
        (MIX, "mix", false),
    ]
};

/// One stored parameter: its index, preset key and whether it only takes
/// whole numbers, as switches and choices do.
type PresetParam = (usize, &'static str, bool);

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

fn parameter_keys(effect: &UiEffect) -> Result<Vec<PresetParam>, String> {
    if effect.plugin_ref.is_some() || effect.plugin_name.is_some() {
        return Err("Use the plugin's own preset controls for this device".into());
    }
    match effect.effect_type {
        EffectType::Gain => Ok(GAIN_KEYS
            .iter()
            .enumerate()
            .map(|(index, key)| {
                let whole = vibez_dsp::gain::param::POLARITY.contains(&index);
                (index, *key, whole)
            })
            .collect()),
        EffectType::Eq => Ok(EQ_KEYS.to_vec()),
        EffectType::Compressor => Ok(COMPRESSOR_KEYS.to_vec()),
        EffectType::Filter => Ok(FILTER_KEYS.to_vec()),
        EffectType::Gate => Ok(GATE_KEYS.to_vec()),
        _ => Err("Presets are not yet supported by this built-in device".into()),
    }
}

pub fn capture(effect: &UiEffect, name: &str) -> Result<DevicePreset, String> {
    let parameters = parameter_keys(effect)?
        .into_iter()
        .map(|(index, key, _)| {
            let descriptor = effect
                .descriptors
                .get(index)
                .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
            let value = effect
                .params
                .get(index)
                .copied()
                .unwrap_or(descriptor.default);
            Ok((key.to_owned(), value))
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
    for (index, key, whole) in keys {
        let value = *preset
            .parameters()
            .get(key)
            .ok_or_else(|| format!("The preset is missing parameter {key}"))?;
        let descriptor = effect
            .descriptors
            .get(index)
            .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
        if !value.is_finite()
            || !(descriptor.min..=descriptor.max).contains(&value)
            || (whole && value.fract() != 0.0)
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
    match effect_type {
        EffectType::Gain => gain_factory_presets(),
        EffectType::Eq => eq_factory_presets(),
        EffectType::Compressor => compressor_factory_presets(),
        EffectType::Filter => filter_factory_presets(),
        EffectType::Gate => gate_factory_presets(),
        _ => Vec::new(),
    }
}

fn gain_factory_presets() -> Vec<DevicePreset> {
    let effect_type = EffectType::Gain;
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

/// Starting points for common mixing jobs, each a change from flat.
fn eq_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Flat", &[]),
        (
            "Vocal presence",
            &[
                ("low_cut_enabled", 1.0),
                ("low_cut_hz", 90.0),
                ("lmf_gain_db", -2.0),
                ("lmf_frequency_hz", 300.0),
                ("lmf_q", 1.0),
                ("hmf_gain_db", 3.0),
                ("hmf_frequency_hz", 3500.0),
                ("hmf_q", 0.9),
                ("hf_gain_db", 2.0),
                ("hf_frequency_hz", 10_000.0),
            ],
        ),
        (
            "Kick punch",
            &[
                ("lf_bell", 1.0),
                ("lf_gain_db", 3.0),
                ("lf_frequency_hz", 60.0),
                ("lmf_gain_db", -4.0),
                ("lmf_frequency_hz", 350.0),
                ("lmf_q", 1.4),
                ("hmf_gain_db", 2.0),
                ("hmf_frequency_hz", 3000.0),
                ("hmf_q", 1.2),
            ],
        ),
        (
            "Tight bass",
            &[
                ("low_cut_enabled", 1.0),
                ("low_cut_hz", 35.0),
                ("lf_gain_db", 2.0),
                ("lf_frequency_hz", 80.0),
                ("lmf_gain_db", -3.0),
                ("lmf_frequency_hz", 250.0),
                ("lmf_q", 1.0),
            ],
        ),
        (
            "Clean hats",
            &[
                ("low_cut_enabled", 1.0),
                ("low_cut_hz", 300.0),
                ("hmf_gain_db", -2.0),
                ("hmf_frequency_hz", 5500.0),
                ("hmf_q", 1.5),
                ("hf_gain_db", 2.0),
                ("hf_frequency_hz", 12_000.0),
            ],
        ),
        (
            "Mud cut",
            &[
                ("lmf_gain_db", -4.0),
                ("lmf_frequency_hz", 300.0),
                ("lmf_q", 1.2),
            ],
        ),
        (
            "Bus air",
            &[
                ("lf_gain_db", 1.0),
                ("lf_frequency_hz", 50.0),
                ("hf_gain_db", 1.5),
                ("hf_frequency_hz", 12_000.0),
            ],
        ),
        (
            "Telephone",
            &[
                ("low_cut_enabled", 1.0),
                ("low_cut_hz", 400.0),
                ("high_cut_enabled", 1.0),
                ("high_cut_hz", 3500.0),
                ("hmf_gain_db", 4.0),
                ("hmf_frequency_hz", 1500.0),
                ("hmf_q", 1.0),
                ("output_db", 3.0),
            ],
        ),
    ];
    keyed_presets(EffectType::Eq, &EQ_KEYS, vibez_dsp::eq::EQ_PARAMS, &presets)
}

/// Starting points for common dynamics jobs, after Sol's tuning.
fn compressor_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Clean bus",
            &[
                ("threshold_db", -16.0),
                ("ratio", 2.0),
                ("attack_ms", 20.0),
                ("release_ms", 150.0),
                ("makeup_db", 1.0),
                ("knee_db", 6.0),
                ("hp_enabled", 1.0),
            ],
        ),
        (
            "Drum punch",
            &[
                ("threshold_db", -18.0),
                ("ratio", 4.0),
                ("attack_ms", 25.0),
                ("release_ms", 80.0),
                ("knee_db", 3.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 60.0),
            ],
        ),
        (
            "Vocal level",
            &[
                ("threshold_db", -24.0),
                ("ratio", 3.0),
                ("attack_ms", 4.0),
                ("release_ms", 120.0),
                ("makeup_db", 5.0),
                ("knee_db", 9.0),
                ("detector_mode", 1.0),
                ("auto_release", 1.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 100.0),
            ],
        ),
        (
            "Gentle glue",
            &[
                ("threshold_db", -16.0),
                ("ratio", 1.8),
                ("attack_ms", 30.0),
                ("release_ms", 200.0),
                ("makeup_db", 1.0),
                ("knee_db", 12.0),
                ("auto_release", 1.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 90.0),
            ],
        ),
        (
            "Parallel crush",
            &[
                ("threshold_db", -30.0),
                ("ratio", 10.0),
                ("attack_ms", 15.0),
                ("release_ms", 140.0),
                ("makeup_db", 12.0),
                ("mix", 0.25),
                ("knee_db", 3.0),
                ("range_enabled", 1.0),
                ("range_db", 24.0),
            ],
        ),
        (
            "Kick duck",
            &[
                ("threshold_db", -24.0),
                ("ratio", 8.0),
                ("attack_ms", 2.0),
                ("release_ms", 140.0),
                ("makeup_db", 0.0),
                ("knee_db", 3.0),
                ("lp_enabled", 1.0),
                ("lp_frequency_hz", 1800.0),
            ],
        ),
        (
            "Smooth limit",
            &[
                ("threshold_db", -6.0),
                ("ratio", 20.0),
                ("attack_ms", 1.0),
                ("release_ms", 60.0),
                ("knee_db", 2.0),
                ("auto_release", 1.0),
            ],
        ),
    ];
    keyed_presets(
        EffectType::Compressor,
        &COMPRESSOR_KEYS,
        vibez_dsp::compressor::COMPRESSOR_PARAMS,
        &presets,
    )
}

/// Sol's starting points, with Gentle Sweep moving as its name and the
/// concept say.
fn filter_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Gentle Sweep",
            &[
                ("cutoff_hz", 1200.0),
                ("slope", 1.0),
                ("mod_source", 1.0),
                ("sync", 3.0),
                ("depth_octaves", 1.1),
            ],
        ),
        (
            "Subsonic Cut",
            &[("cutoff_hz", 80.0), ("mode", 1.0), ("slope", 1.0)],
        ),
        (
            "Telephone",
            &[
                ("resonance_q", 0.5),
                ("mode", 2.0),
                ("drive_db", 2.0),
                ("mix", 0.8),
            ],
        ),
        (
            "Resonant Edge",
            &[("cutoff_hz", 2200.0), ("resonance_q", 3.0)],
        ),
        (
            "Hum Notch",
            &[
                ("cutoff_hz", 50.0),
                ("resonance_q", 5.0),
                ("mode", 3.0),
                ("slope", 1.0),
            ],
        ),
        (
            "Envelope Wah",
            &[
                ("cutoff_hz", 400.0),
                ("resonance_q", 1.0),
                ("slope", 1.0),
                ("mod_source", 2.0),
                ("depth_octaves", 3.0),
                ("envelope_release_ms", 200.0),
            ],
        ),
        (
            "Stereo Eighths",
            &[
                ("cutoff_hz", 1800.0),
                ("resonance_q", 0.9),
                ("slope", 1.0),
                ("mod_source", 1.0),
                ("depth_octaves", 2.0),
                ("sync", 4.0),
                ("stereo_phase_degrees", 90.0),
            ],
        ),
    ];
    keyed_presets(
        EffectType::Filter,
        &FILTER_KEYS,
        vibez_dsp::filter::FILTER_PARAMS,
        &presets,
    )
}

/// Sol's starting points for drums, vocals and keyed gating.
fn gate_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Tight Drums",
            &[
                ("threshold_db", -32.0),
                ("hold_ms", 60.0),
                ("release_ms", 120.0),
                ("hysteresis_db", 6.0),
                ("range_enabled", 1.0),
                ("range_db", 60.0),
            ],
        ),
        (
            "Gentle Cleanup",
            &[
                ("threshold_db", -48.0),
                ("attack_ms", 3.0),
                ("hold_ms", 30.0),
                ("release_ms", 180.0),
                ("hysteresis_db", 4.0),
                ("range_enabled", 1.0),
                ("range_db", 18.0),
            ],
        ),
        (
            "Snare Body",
            &[
                ("threshold_db", -28.0),
                ("attack_ms", 0.5),
                ("hold_ms", 85.0),
                ("release_ms", 170.0),
                ("hysteresis_db", 5.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 160.0),
                ("lp_enabled", 1.0),
                ("lp_frequency_hz", 6000.0),
                ("range_enabled", 1.0),
                ("range_db", 48.0),
            ],
        ),
        (
            "Short Percussion",
            &[
                ("threshold_db", -34.0),
                ("attack_ms", 0.2),
                ("hold_ms", 12.0),
                ("release_ms", 35.0),
                ("hysteresis_db", 3.0),
                ("range_enabled", 1.0),
                ("range_db", 72.0),
            ],
        ),
        (
            "Vocal Expander",
            &[
                ("threshold_db", -42.0),
                ("attack_ms", 4.0),
                ("hold_ms", 35.0),
                ("release_ms", 210.0),
                ("hysteresis_db", 2.0),
                ("mode", 1.0),
                ("expansion_ratio", 2.0),
                ("range_enabled", 1.0),
                ("range_db", 15.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 100.0),
            ],
        ),
        (
            "External Trigger",
            &[
                ("threshold_db", -24.0),
                ("attack_ms", 0.2),
                ("hold_ms", 45.0),
                ("release_ms", 90.0),
                ("hysteresis_db", 4.0),
                ("hp_enabled", 1.0),
                ("hp_frequency_hz", 40.0),
                ("lp_enabled", 1.0),
                ("lp_frequency_hz", 1800.0),
            ],
        ),
        (
            "Soft Floor",
            &[
                ("threshold_db", -38.0),
                ("attack_ms", 8.0),
                ("hold_ms", 20.0),
                ("release_ms", 250.0),
                ("hysteresis_db", 5.0),
                ("range_enabled", 1.0),
                ("range_db", 12.0),
            ],
        ),
    ];
    keyed_presets(
        EffectType::Gate,
        &GATE_KEYS,
        vibez_dsp::gate::GATE_PARAMS,
        &presets,
    )
}

/// Presets that start from the device's defaults and change the named keys.
fn keyed_presets(
    effect_type: EffectType,
    keys: &[PresetParam],
    descriptors: &[vibez_core::effect::ParamDescriptor],
    presets: &[(&str, &[(&str, f32)])],
) -> Vec<DevicePreset> {
    presets
        .iter()
        .map(|(name, changes)| {
            debug_assert!(
                changes
                    .iter()
                    .all(|(changed, _)| keys.iter().any(|(_, key, _)| key == changed)),
                "{name} names a parameter the device does not store"
            );
            let parameters = keys
                .iter()
                .map(|(index, key, _)| {
                    let value = changes
                        .iter()
                        .find(|(changed, _)| changed == key)
                        .map_or(descriptors[*index].default, |(_, value)| *value);
                    ((*key).to_owned(), value)
                })
                .collect();
            DevicePreset::new(DevicePresetKind::Effect(effect_type), *name, parameters)
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

    fn eq() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Eq, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Eq,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn eq_factory_presets_are_valid_distinct_and_start_from_flat() {
        let effect = eq();
        let presets = factory_presets(EffectType::Eq);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Flat");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let updates = prepare_updates(&effect, preset).unwrap();
            assert!(!updates.is_empty(), "{} changes nothing", preset.name());
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn eq_presets_leave_the_soloed_band_alone() {
        let audition = vibez_dsp::eq::param::AUDITION;
        let mut effect = eq();
        effect.params[audition] = 3.0;
        let captured = capture(&effect, "Soloed").unwrap();
        effect.params[audition] = 0.0;
        assert_eq!(prepare_updates(&effect, &captured), Ok(Vec::new()));
        for preset in factory_presets(EffectType::Eq) {
            let updates = prepare_updates(&effect, &preset).unwrap();
            assert!(updates.iter().all(|(index, _)| *index != audition));
        }
    }

    #[test]
    fn eq_switches_in_a_preset_must_be_on_or_off() {
        let effect = eq();
        let mut parameters = capture(&effect, "Half").unwrap().parameters().clone();
        parameters.insert("lf_bell".into(), 0.5);
        let half = DevicePreset::new(DevicePresetKind::Effect(EffectType::Eq), "Half", parameters)
            .unwrap();
        assert!(prepare_updates(&effect, &half).is_err());
    }

    fn compressor() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Compressor, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Compressor,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn compressor_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = compressor();
        let presets = factory_presets(EffectType::Compressor);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            assert!(!prepare_updates(&effect, preset).unwrap().is_empty());
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn compressor_presets_leave_detector_listening_alone() {
        let listen = vibez_dsp::compressor::param::DETECTOR_LISTEN;
        let mut effect = compressor();
        effect.params[listen] = 1.0;
        let captured = capture(&effect, "Listening").unwrap();
        effect.params[listen] = 0.0;
        assert_eq!(prepare_updates(&effect, &captured), Ok(Vec::new()));
    }

    fn filter() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Filter, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Filter,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn filter_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = filter();
        let presets = factory_presets(EffectType::Filter);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = filter();
            for (index, value) in prepare_updates(&effect, preset).unwrap() {
                applied.params[index] = value;
            }
            assert_ne!(applied.params, effect.params, "{}", preset.name());
            assert_eq!(capture(&applied, preset.name()).as_ref(), Ok(preset));
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn filter_choices_in_a_preset_must_be_whole() {
        use vibez_dsp::filter::param;
        let effect = filter();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["mode", "slope", "mod_source", "sync"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
        let mut parameters = captured.parameters().clone();
        parameters.insert("sync".into(), 7.0);
        let synced = DevicePreset::new(captured.kind(), "Synced", parameters).unwrap();
        assert_eq!(
            prepare_updates(&effect, &synced),
            Ok(vec![(param::SYNC, 7.0)])
        );
    }

    fn gate() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Gate, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Gate,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn gate_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = gate();
        let presets = factory_presets(EffectType::Gate);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = gate();
            for (index, value) in prepare_updates(&effect, preset).unwrap() {
                applied.params[index] = value;
            }
            assert_ne!(applied.params, effect.params, "{}", preset.name());
            assert_eq!(capture(&applied, preset.name()).as_ref(), Ok(preset));
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn gate_presets_leave_detector_audition_alone() {
        let listen = vibez_dsp::gate::param::DETECTOR_LISTEN;
        let mut effect = gate();
        effect.params[listen] = 1.0;
        let captured = capture(&effect, "Auditioning").unwrap();
        effect.params[listen] = 0.0;
        assert_eq!(prepare_updates(&effect, &captured), Ok(Vec::new()));
        let mut parameters = captured.parameters().clone();
        parameters.insert("mode".into(), 0.5);
        let half = DevicePreset::new(captured.kind(), "Half", parameters).unwrap();
        assert!(prepare_updates(&effect, &half).is_err());
    }

    fn drive() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Drive, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Drive,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn drive_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = drive();
        let presets = factory_presets(EffectType::Drive);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = drive();
            for (index, value) in prepare_updates(&effect, preset).unwrap() {
                applied.params[index] = value;
            }
            assert_ne!(applied.params, effect.params, "{}", preset.name());
            assert_eq!(capture(&applied, preset.name()).as_ref(), Ok(preset));
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn drive_choices_in_a_preset_must_be_whole() {
        let effect = drive();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["character", "quality", "pre_hp_enabled", "post_lp_enabled"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
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
