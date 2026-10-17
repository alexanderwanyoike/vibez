//! Built-in device preset selection and validated parameter changes.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_core::id::TrackId;
use vibez_core::midi::InstrumentKind;

use crate::domains::device_params::{DeviceParams, DeviceSlot};
#[cfg(test)]
use crate::state::UiEffect;

#[derive(Debug, Clone)]
pub enum DevicePresetMsg {
    Load(TrackId, DeviceSlot),
    Save(TrackId, DeviceSlot),
    Loaded(PresetLoadRequest, Result<Option<DevicePreset>, String>),
    Saved(PresetSaveRequest, Result<Option<DevicePreset>, String>),
}

#[derive(Debug, Clone)]
pub struct PresetSaveRequest {
    pub token: u64,
    pub track_id: TrackId,
    pub slot: DeviceSlot,
    pub preset: DevicePreset,
}

#[derive(Debug, Clone)]
pub struct PresetLoadRequest {
    pub token: u64,
    pub track_id: TrackId,
    pub slot: DeviceSlot,
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

/// Every native Drive parameter; its character, oversampling and filter
/// switches are choices.
const DRIVE_KEYS: [(usize, &str, bool); 10] = {
    use vibez_dsp::drive::param::*;
    [
        (AMOUNT, "amount", false),
        (TONE, "tone", false),
        (OUTPUT, "output", false),
        (MIX, "mix", false),
        (CHARACTER, "character", true),
        (QUALITY, "quality", true),
        (PRE_HP_ENABLED, "pre_hp_enabled", true),
        (PRE_HP_FREQUENCY, "pre_hp_frequency_hz", false),
        (POST_LP_ENABLED, "post_lp_enabled", true),
        (POST_LP_FREQUENCY, "post_lp_frequency_hz", false),
    ]
};

/// Every native Bitcrush parameter; its rounding, dither and filter
/// switches are choices.
const BITCRUSH_KEYS: [(usize, &str, bool); 9] = {
    use vibez_dsp::bitcrush::param::*;
    [
        (BITS, "bits", false),
        (SR_REDUCE, "sr_reduce", false),
        (MIX, "mix", false),
        (DRIVE_DB, "drive_db", false),
        (ROUNDING, "rounding", true),
        (DITHER, "dither", true),
        (JITTER, "jitter", false),
        (POST_LP_ENABLED, "post_lp_enabled", true),
        (POST_LP_HZ, "post_lp_frequency_hz", false),
    ]
};

/// Every native Delay parameter; its sync, divisions, modifiers, routing,
/// link, filter switches and time change are choices.
const DELAY_KEYS: [(usize, &str, bool); 17] = {
    use vibez_dsp::delay::param::*;
    [
        (TIME_MS, "time_ms", false),
        (FEEDBACK, "feedback", false),
        (MIX, "mix", false),
        (SYNC, "sync", true),
        (LEFT_DIVISION, "left_division", true),
        (LEFT_MODIFIER, "left_modifier", true),
        (ROUTING, "routing", true),
        (LINK, "link", true),
        (RIGHT_TIME_MS, "right_time_ms", false),
        (RIGHT_DIVISION, "right_division", true),
        (RIGHT_MODIFIER, "right_modifier", true),
        (FEEDBACK_HP_ENABLED, "feedback_hp_enabled", true),
        (FEEDBACK_HP_HZ, "feedback_hp_hz", false),
        (FEEDBACK_LP_ENABLED, "feedback_lp_enabled", true),
        (FEEDBACK_LP_HZ, "feedback_lp_hz", false),
        (DUCK, "duck", false),
        (TIME_CHANGE, "time_change", true),
    ]
};

/// Every native Echo parameter.
const ECHO_KEYS: [(usize, &str, bool); 17] = {
    use vibez_dsp::echo::param::*;
    [
        (TIME_MS, "time_ms", false),
        (SYNC, "sync", true),
        (GRID, "grid", true),
        (FEEDBACK, "feedback", false),
        (MIX, "mix", false),
        (MODE, "mode", true),
        (CHARACTER, "character", false),
        (TONE, "tone", false),
        (STYLE, "tape_character", true),
        (WEAR, "wear", false),
        (WOW, "wow", false),
        (FLUTTER, "flutter", false),
        (SATURATION, "saturation", false),
        (DIFFUSION, "diffusion", false),
        (DUCK, "duck", false),
        (LOW_CUT, "low_cut", false),
        (HIGH_CUT, "high_cut", false),
    ]
};

/// Every native Saturator parameter.
const SATURATOR_KEYS: [(usize, &str, bool); 15] = {
    use vibez_dsp::saturator::param::*;
    [
        (DRIVE_DB, "drive_db", false),
        (BIAS, "bias", false),
        (OUTPUT_DB, "output_db", false),
        (MIX, "mix", false),
        (AUTO_GAIN, "auto_gain", true),
        (SHAPE, "shape", true),
        (CURVE, "curve", false),
        (ASYMMETRY, "asymmetry", false),
        (PRE_FILTER, "pre_filter", true),
        (POST_FILTER, "post_filter", true),
        (DC_REMOVAL, "dc_removal", true),
        (QUALITY, "quality", true),
        (CUSTOM_LOW, "custom_low_db", false),
        (CUSTOM_MID, "custom_mid_db", false),
        (CUSTOM_HIGH, "custom_high_db", false),
    ]
};

/// Every native Reverb parameter; its sync and freeze are choices.
const REVERB_KEYS: [(usize, &str, bool); 10] = {
    use vibez_dsp::reverb::param::*;
    [
        (ROOM_SIZE, "room_size", false),
        (DAMPING, "damping", false),
        (MIX, "mix", false),
        (PREDELAY_MS, "predelay_ms", false),
        (PREDELAY_SYNC, "predelay_sync", true),
        (WIDTH, "width", false),
        (DIFFUSION, "diffusion", false),
        (LOW_CUT_HZ, "low_cut_hz", false),
        (HIGH_CUT_HZ, "high_cut_hz", false),
        (FREEZE, "freeze", true),
    ]
};

const PHASER_KEYS: [(usize, &str, bool); 10] = {
    use vibez_dsp::phaser::param::*;
    [
        (RATE, "rate", false),
        (DEPTH, "depth", false),
        (FEEDBACK, "feedback", false),
        (MIX, "mix", false),
        (CENTRE, "centre", false),
        (STAGES, "stages", true),
        (STEREO_PHASE, "stereo_phase", false),
        (POLARITY, "polarity", true),
        (SHAPE, "shape", true),
        (SYNC, "sync", true),
    ]
};

/// Shape is stored as it is, since older projects can sit between two
/// shapes.
const AUTO_PAN_KEYS: [(usize, &str, bool); 9] = {
    use vibez_dsp::auto_pan::param::*;
    [
        (RATE, "rate", false),
        (DEPTH, "depth", false),
        (SHAPE, "shape", false),
        (MODE, "mode", true),
        (PHASE, "phase", false),
        (OFFSET, "offset", false),
        (SMOOTHING, "smoothing", false),
        (SYNC, "sync", true),
        (RETRIGGER, "retrigger", true),
    ]
};

/// Every Subtractive Synth control. Shapes, types, modes and counts are
/// whole.
const SYNTH_KEYS: [&str; vibez_instruments::synth::param::COUNT] = [
    "waveform",
    "attack",
    "decay",
    "sustain",
    "release",
    "cutoff",
    "resonance",
    "volume",
    "octave",
    "detune",
    "waveform_2",
    "octave_2",
    "detune_2",
    "level_2",
    "sub",
    "noise",
    "filter_type",
    "voice_mode",
    "voices",
    "glide",
    "lfo_shape",
    "lfo_target",
    "lfo_rate",
    "lfo_amount",
    "filter_env",
    "filter_attack",
    "filter_decay",
    "filter_sustain",
    "filter_release",
];

/// How a Sampler plays its sample. The root, trim and loop points belong
/// to the sample itself, so a preset leaves them where they are.
const SAMPLER_KEYS: [(usize, &str); 29] = {
    use vibez_instruments::sampler::param::*;
    [
        (ATTACK, "attack"),
        (DECAY, "decay"),
        (SUSTAIN, "sustain"),
        (RELEASE, "release"),
        (LOOP, "loop"),
        (ONE_SHOT, "one_shot"),
        (VOLUME, "volume"),
        (REVERSE, "reverse"),
        (CROSSFADE, "crossfade"),
        (TUNE, "tune"),
        (FINE, "fine"),
        (GLIDE, "glide"),
        (KEY_LOW, "key_low"),
        (KEY_HIGH, "key_high"),
        (VELOCITY_LOW, "velocity_low"),
        (VELOCITY_HIGH, "velocity_high"),
        (VELOCITY_AMOUNT, "velocity_amount"),
        (FILTER_TYPE, "filter_type"),
        (CUTOFF, "cutoff"),
        (RESONANCE, "resonance"),
        (FILTER_ENV, "filter_env"),
        (FILTER_ATTACK, "filter_attack"),
        (FILTER_DECAY, "filter_decay"),
        (FILTER_SUSTAIN, "filter_sustain"),
        (FILTER_RELEASE, "filter_release"),
        (LFO_SHAPE, "lfo_shape"),
        (LFO_TARGET, "lfo_target"),
        (LFO_RATE, "lfo_rate"),
        (LFO_AMOUNT, "lfo_amount"),
    ]
};

fn sampler_keys() -> Vec<PresetParam> {
    SAMPLER_KEYS
        .iter()
        .map(|(index, key)| (*index, *key, vibez_instruments::sampler::is_whole(*index)))
        .collect()
}

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

pub fn presentation<'a>(
    device: impl Into<DeviceParams<'a>>,
    remembered: Option<&DevicePreset>,
) -> PresetPresentation {
    let device = device.into();
    let presets = presets_for(device.kind);
    let selected = remembered.cloned().or_else(|| {
        presets
            .iter()
            .find(|preset| prepare_updates(device, preset).is_ok_and(|updates| updates.is_empty()))
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
        prepare_updates(device, preset).map_or(true, |updates| !updates.is_empty())
    });
    PresetPresentation {
        options: presets,
        selected,
        modified,
        previous,
        next,
    }
}

fn parameter_keys(device: &DeviceParams) -> Result<Vec<PresetParam>, String> {
    if device.plugin {
        return Err("Use the plugin's own preset controls for this device".into());
    }
    let effect_type = match device.kind {
        DevicePresetKind::Effect(effect_type) => effect_type,
        DevicePresetKind::Instrument(InstrumentKind::SubtractiveSynth) => {
            return Ok(synth_keys());
        }
        DevicePresetKind::Instrument(InstrumentKind::Sampler) => {
            return Ok(sampler_keys());
        }
        DevicePresetKind::Instrument(_) => {
            return Err("Presets are not yet supported by this built-in device".into());
        }
    };
    match effect_type {
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
        EffectType::Drive => Ok(DRIVE_KEYS.to_vec()),
        EffectType::Bitcrush => Ok(BITCRUSH_KEYS.to_vec()),
        EffectType::Delay => Ok(DELAY_KEYS.to_vec()),
        EffectType::Echo => Ok(ECHO_KEYS.to_vec()),
        EffectType::Saturator => Ok(SATURATOR_KEYS.to_vec()),
        EffectType::Reverb => Ok(REVERB_KEYS.to_vec()),
        EffectType::AutoPan => Ok(AUTO_PAN_KEYS.to_vec()),
        EffectType::Phaser => Ok(PHASER_KEYS.to_vec()),
    }
}

pub fn capture<'a>(
    device: impl Into<DeviceParams<'a>>,
    name: &str,
) -> Result<DevicePreset, String> {
    let device = device.into();
    let parameters = parameter_keys(&device)?
        .into_iter()
        .map(|(index, key, _)| {
            let descriptor = device
                .descriptors
                .get(index)
                .ok_or_else(|| "The device's parameters are unavailable".to_string())?;
            let value = device
                .params
                .get(index)
                .copied()
                .unwrap_or(descriptor.default);
            Ok((key.to_owned(), value))
        })
        .collect::<Result<_, String>>()?;
    DevicePreset::new(device.kind, name, parameters).map_err(|error| error.to_string())
}

pub fn prepare_updates<'a>(
    device: impl Into<DeviceParams<'a>>,
    preset: &DevicePreset,
) -> Result<Vec<(usize, f32)>, String> {
    let device = device.into();
    let keys = parameter_keys(&device)?;
    if preset.kind() != device.kind {
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
        let descriptor = device
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
        if device
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

/// Every built-in device's factory presets, by kind.
pub fn presets_for(kind: DevicePresetKind) -> Vec<DevicePreset> {
    match kind {
        DevicePresetKind::Effect(effect_type) => factory_presets(effect_type),
        DevicePresetKind::Instrument(InstrumentKind::SubtractiveSynth) => synth_factory_presets(),
        DevicePresetKind::Instrument(InstrumentKind::Sampler) => sampler_factory_presets(),
        DevicePresetKind::Instrument(_) => Vec::new(),
    }
}

fn sampler_factory_presets() -> Vec<DevicePreset> {
    use vibez_instruments::sampler::filter_type;
    use vibez_instruments::synth::{lfo_shape, lfo_target};
    let presets: [(&str, &[(&str, f32)]); 9] = [
        ("Init", &[]),
        (
            "Warm Rhodes",
            &[
                ("attack", 0.012),
                ("decay", 0.18),
                ("sustain", 0.5),
                ("release", 1.2),
                ("volume", 0.7),
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 3_200.0),
                ("filter_env", 1.5),
                ("filter_decay", 0.4),
                ("velocity_amount", 0.7),
            ],
        ),
        (
            "Pluck",
            &[
                ("decay", 0.25),
                ("sustain", 0.0),
                ("release", 0.15),
                ("filter_type", filter_type::LP12 as f32),
                ("cutoff", 900.0),
                ("resonance", 1.5),
                ("filter_env", 3.0),
                ("filter_decay", 0.15),
            ],
        ),
        (
            "Looped Pad",
            &[
                ("loop", 1.0),
                ("crossfade", 40.0),
                ("attack", 0.4),
                ("release", 2.0),
                ("filter_type", filter_type::LP12 as f32),
                ("cutoff", 2_500.0),
                ("lfo_rate", 0.3),
                ("lfo_amount", 0.15),
            ],
        ),
        (
            "Reverse Swell",
            &[("reverse", 1.0), ("attack", 1.0), ("release", 0.4)],
        ),
        (
            "One-shot Hit",
            &[
                ("one_shot", 1.0),
                ("release", 0.05),
                ("velocity_amount", 0.6),
            ],
        ),
        (
            "Lo-Fi",
            &[
                ("filter_type", filter_type::LP12 as f32),
                ("cutoff", 2_200.0),
                ("resonance", 1.2),
                ("fine", -15.0),
                ("lfo_target", lfo_target::PITCH as f32),
                ("lfo_rate", 0.7),
                ("lfo_amount", 0.008),
            ],
        ),
        (
            "Wobble",
            &[
                ("loop", 1.0),
                ("crossfade", 20.0),
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 400.0),
                ("resonance", 3.0),
                ("lfo_rate", 3.0),
                ("lfo_amount", 0.6),
            ],
        ),
        (
            "Tremolo",
            &[
                ("lfo_shape", lfo_shape::TRIANGLE as f32),
                ("lfo_target", lfo_target::VOLUME as f32),
                ("lfo_rate", 5.0),
                ("lfo_amount", 0.5),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Instrument(InstrumentKind::Sampler),
        &sampler_keys(),
        vibez_instruments::sampler::SAMPLER_PARAMS,
        &presets,
    )
}

fn synth_keys() -> Vec<PresetParam> {
    SYNTH_KEYS
        .iter()
        .enumerate()
        .map(|(index, key)| (index, *key, vibez_instruments::synth::is_whole(index)))
        .collect()
}

fn synth_factory_presets() -> Vec<DevicePreset> {
    use vibez_instruments::synth::{filter_type, lfo_shape, lfo_target, voice_mode, Waveform};
    let wave = |waveform: Waveform| waveform as usize as f32;
    let presets: [(&str, &[(&str, f32)]); 9] = [
        ("Init", &[]),
        (
            "Soft Analog",
            &[
                ("waveform_2", wave(Waveform::Triangle)),
                ("octave_2", -1.0),
                ("level_2", 0.6),
                ("sub", 0.25),
                ("noise", 0.063),
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 2_400.0),
                ("resonance", 1.5),
                ("attack", 0.012),
                ("decay", 0.24),
                ("sustain", 0.68),
                ("release", 0.42),
                ("lfo_amount", 0.35),
                ("volume", 0.5),
            ],
        ),
        (
            "Pluck",
            &[
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 600.0),
                ("resonance", 2.0),
                ("filter_env", 3.5),
                ("filter_decay", 0.18),
                ("attack", 0.002),
                ("decay", 0.35),
                ("sustain", 0.0),
                ("release", 0.2),
            ],
        ),
        (
            "Warm Bass",
            &[
                ("waveform_2", wave(Waveform::Square)),
                ("octave_2", -1.0),
                ("level_2", 0.4),
                ("sub", 0.6),
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 400.0),
                ("resonance", 1.2),
                ("filter_env", 2.0),
                ("filter_decay", 0.25),
                ("voice_mode", voice_mode::MONO as f32),
                ("glide", 40.0),
                ("attack", 0.003),
                ("decay", 0.3),
                ("sustain", 0.8),
                ("release", 0.1),
            ],
        ),
        (
            "Legato Lead",
            &[
                ("waveform_2", wave(Waveform::Saw)),
                ("detune_2", 0.12),
                ("level_2", 0.8),
                ("cutoff", 3_000.0),
                ("resonance", 2.0),
                ("voice_mode", voice_mode::LEGATO as f32),
                ("glide", 80.0),
                ("lfo_target", lfo_target::PITCH as f32),
                ("lfo_rate", 5.5),
                ("lfo_amount", 0.02),
                ("release", 0.25),
            ],
        ),
        (
            "Slow Pad",
            &[
                ("waveform_2", wave(Waveform::Saw)),
                ("detune_2", -0.1),
                ("level_2", 1.0),
                ("cutoff", 1_800.0),
                ("attack", 0.8),
                ("decay", 1.0),
                ("sustain", 0.8),
                ("release", 2.5),
                ("lfo_rate", 0.3),
                ("lfo_amount", 0.2),
                ("volume", 0.5),
            ],
        ),
        (
            "Wobble Bass",
            &[
                ("sub", 0.5),
                ("filter_type", filter_type::LP24 as f32),
                ("cutoff", 300.0),
                ("resonance", 4.0),
                ("voice_mode", voice_mode::MONO as f32),
                ("lfo_rate", 3.0),
                ("lfo_amount", 0.6),
            ],
        ),
        (
            "Square Keys",
            &[
                ("waveform", wave(Waveform::Square)),
                ("cutoff", 2_500.0),
                ("filter_env", 1.5),
                ("filter_decay", 0.4),
                ("attack", 0.002),
                ("decay", 0.6),
                ("sustain", 0.3),
            ],
        ),
        (
            "Noise Sweep",
            &[
                ("waveform", wave(Waveform::Sine)),
                ("noise", 1.0),
                ("filter_type", filter_type::BP12 as f32),
                ("cutoff", 800.0),
                ("resonance", 5.0),
                ("lfo_shape", lfo_shape::TRIANGLE as f32),
                ("lfo_rate", 0.2),
                ("lfo_amount", 0.7),
                ("attack", 0.5),
                ("release", 1.5),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Instrument(InstrumentKind::SubtractiveSynth),
        &synth_keys(),
        vibez_instruments::synth::SYNTH_PARAMS,
        &presets,
    )
}

pub fn factory_presets(effect_type: EffectType) -> Vec<DevicePreset> {
    match effect_type {
        EffectType::Gain => gain_factory_presets(),
        EffectType::Eq => eq_factory_presets(),
        EffectType::Compressor => compressor_factory_presets(),
        EffectType::Filter => filter_factory_presets(),
        EffectType::Gate => gate_factory_presets(),
        EffectType::Drive => drive_factory_presets(),
        EffectType::Bitcrush => bitcrush_factory_presets(),
        EffectType::Delay => delay_factory_presets(),
        EffectType::Echo => echo_factory_presets(),
        EffectType::Saturator => saturator_factory_presets(),
        EffectType::Reverb => reverb_factory_presets(),
        EffectType::AutoPan => auto_pan_factory_presets(),
        EffectType::Phaser => phaser_factory_presets(),
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
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Eq),
        &EQ_KEYS,
        vibez_dsp::eq::EQ_PARAMS,
        &presets,
    )
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
        DevicePresetKind::Effect(EffectType::Compressor),
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
        DevicePresetKind::Effect(EffectType::Filter),
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
        DevicePresetKind::Effect(EffectType::Gate),
        &GATE_KEYS,
        vibez_dsp::gate::GATE_PARAMS,
        &presets,
    )
}

/// Sol's starting points, his drive amounts given as dB of gain into the
/// shaper.
fn drive_factory_presets() -> Vec<DevicePreset> {
    use vibez_dsp::drive::character::{HARD, SOFT, WARM};
    let amount =
        |db: f32| vibez_dsp::drive::amount_for_gain(vibez_core::level::db_to_amplitude(db));
    let level = vibez_core::level::db_to_amplitude;
    let presets: [(&str, Vec<(&str, f32)>); 8] = [
        ("Init", vec![]),
        (
            "Warm Edge",
            vec![
                ("character", SOFT as f32),
                ("amount", amount(6.0)),
                ("tone", 0.55),
                ("output", level(-3.0)),
                ("mix", 0.8),
            ],
        ),
        (
            "Drum Bite",
            vec![
                ("character", WARM as f32),
                ("amount", amount(16.0)),
                ("tone", 0.6),
                ("output", level(-6.0)),
                ("mix", 0.7),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 40.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 16_000.0),
            ],
        ),
        (
            "Soft Bass",
            vec![
                ("character", SOFT as f32),
                ("amount", amount(10.0)),
                ("tone", 0.38),
                ("output", level(-4.0)),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 25.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 8_000.0),
            ],
        ),
        (
            "Bright Percussion",
            vec![
                ("character", SOFT as f32),
                ("amount", amount(12.0)),
                ("tone", 0.65),
                ("output", level(-5.0)),
                ("mix", 0.6),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 120.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 18_000.0),
            ],
        ),
        (
            "Parallel Crunch",
            vec![
                ("character", HARD as f32),
                ("amount", amount(24.0)),
                ("tone", 0.55),
                ("output", level(-8.0)),
                ("mix", 0.35),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 80.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 10_000.0),
            ],
        ),
        (
            "Controlled Hard",
            vec![
                ("character", HARD as f32),
                ("amount", amount(18.0)),
                ("output", level(-6.0)),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 40.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 8_500.0),
            ],
        ),
        (
            "Edge Lead",
            vec![
                ("character", WARM as f32),
                ("amount", amount(14.0)),
                ("tone", 0.6),
                ("output", level(-5.0)),
                ("mix", 0.85),
                ("pre_hp_enabled", 1.0),
                ("pre_hp_frequency_hz", 100.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 14_000.0),
            ],
        ),
    ];
    let presets: Vec<(&str, &[(&str, f32)])> = presets
        .iter()
        .map(|(name, changes)| (*name, changes.as_slice()))
        .collect();
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Drive),
        &DRIVE_KEYS,
        vibez_dsp::drive::DRIVE_PARAMS,
        &presets,
    )
}

/// Sol's lo-fi starting points; the rates are dividers of the host rate.
fn bitcrush_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Crunchy 12",
            &[
                ("bits", 12.0),
                ("sr_reduce", 4.0),
                ("mix", 0.65),
                ("drive_db", 2.0),
            ],
        ),
        (
            "Drum Crunch",
            &[
                ("bits", 8.0),
                ("sr_reduce", 3.0),
                ("mix", 0.55),
                ("drive_db", 6.0),
            ],
        ),
        (
            "Dusty Bass",
            &[
                ("sr_reduce", 6.0),
                ("mix", 0.7),
                ("drive_db", 3.0),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 3500.0),
            ],
        ),
        (
            "Pocket Sampler",
            &[("bits", 8.0), ("sr_reduce", 4.0), ("dither", 1.0)],
        ),
        (
            "Broken Clock",
            &[
                ("bits", 12.0),
                ("sr_reduce", 8.0),
                ("mix", 0.75),
                ("drive_db", 2.0),
                ("jitter", 0.35),
                ("post_lp_enabled", 1.0),
                ("post_lp_frequency_hz", 6000.0),
            ],
        ),
        (
            "Parallel Arcade",
            &[
                ("bits", 4.0),
                ("sr_reduce", 10.0),
                ("mix", 0.3),
                ("drive_db", 8.0),
                ("rounding", 2.0),
            ],
        ),
        (
            "Digital Air",
            &[("bits", 6.0), ("mix", 0.2), ("dither", 1.0)],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Bitcrush),
        &BITCRUSH_KEYS,
        vibez_dsp::bitcrush::BITCRUSH_PARAMS,
        &presets,
    )
}

/// Sol's echoes, from a short slap to a long space.
fn echo_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Worn Tape",
            &[
                ("character", 0.55),
                ("tone", 0.4),
                ("wear", 0.45),
                ("wow", 0.2),
                ("flutter", 0.12),
                ("saturation", 0.25),
                ("high_cut", 4500.0),
            ],
        ),
        (
            "Clean Slap",
            &[
                ("tape_character", 0.0),
                ("sync", 0.0),
                ("time_ms", 110.0),
                ("feedback", 0.15),
                ("mix", 0.2),
                ("low_cut", 120.0),
                ("high_cut", 12000.0),
            ],
        ),
        (
            "Dub Space",
            &[
                ("mode", 1.0),
                ("feedback", 0.7),
                ("mix", 0.3),
                ("tone", 0.35),
                ("saturation", 0.3),
                ("duck", 0.35),
                ("low_cut", 250.0),
                ("high_cut", 3500.0),
            ],
        ),
        (
            "Diffuse Wash",
            &[
                ("tape_character", 2.0),
                ("grid", 16.0),
                ("feedback", 0.65),
                ("character", 0.6),
                ("diffusion", 0.7),
                ("mix", 0.35),
                ("high_cut", 7000.0),
            ],
        ),
        (
            "Cassette Eighths",
            &[
                ("grid", 10.0),
                ("feedback", 0.4),
                ("character", 0.6),
                ("wear", 0.6),
                ("wow", 0.35),
                ("flutter", 0.2),
                ("saturation", 0.4),
                ("high_cut", 3800.0),
            ],
        ),
        (
            "Ping-Pong Sixteenths",
            &[
                ("tape_character", 0.0),
                ("mode", 1.0),
                ("grid", 7.0),
                ("feedback", 0.45),
                ("mix", 0.22),
            ],
        ),
        (
            "Ducked Vocal",
            &[
                ("grid", 12.0),
                ("feedback", 0.35),
                ("duck", 0.7),
                ("mix", 0.3),
                ("low_cut", 300.0),
                ("high_cut", 8000.0),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Echo),
        &ECHO_KEYS,
        vibez_dsp::echo::ECHO_PARAMS,
        &presets,
    )
}

fn saturator_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Soft Harmonics",
            &[
                ("drive_db", 9.0),
                ("curve", 0.6),
                ("auto_gain", 1.0),
                ("output_db", 0.0),
            ],
        ),
        (
            "Warm Tube",
            &[
                ("shape", 1.0),
                ("drive_db", 12.0),
                ("asymmetry", 0.2),
                ("auto_gain", 1.0),
                ("output_db", 0.0),
                ("post_filter", 3.0),
            ],
        ),
        (
            "Even Glow",
            &[
                ("shape", 1.0),
                ("drive_db", 6.0),
                ("bias", 0.3),
                ("mix", 0.6),
                ("output_db", 0.0),
            ],
        ),
        (
            "Bass Grit",
            &[
                ("drive_db", 18.0),
                ("curve", 1.0),
                ("pre_filter", 2.0),
                ("post_filter", 4.0),
                ("mix", 0.7),
                ("auto_gain", 1.0),
                ("output_db", 0.0),
            ],
        ),
        (
            "Hard Clip",
            &[
                ("drive_db", 24.0),
                ("curve", 1.0),
                ("auto_gain", 1.0),
                ("output_db", -1.0),
            ],
        ),
        (
            "Wavefolder",
            &[
                ("shape", 2.0),
                ("drive_db", 9.0),
                ("curve", 0.5),
                ("post_filter", 2.0),
                ("output_db", -4.0),
            ],
        ),
        (
            "Crushed Custom",
            &[
                ("shape", 3.0),
                ("drive_db", 12.0),
                ("custom_low_db", -12.0),
                ("custom_mid_db", 3.0),
                ("custom_high_db", 4.0),
                ("output_db", -6.0),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Saturator),
        &SATURATOR_KEYS,
        vibez_dsp::saturator::SATURATOR_PARAMS,
        &presets,
    )
}

fn delay_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Dotted Bounce",
            &[
                ("feedback", 0.35),
                ("mix", 0.22),
                ("sync", 1.0),
                ("left_division", 3.0),
                ("left_modifier", 1.0),
                ("right_division", 3.0),
                ("right_modifier", 1.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_lp_enabled", 1.0),
                ("duck", 0.2),
            ],
        ),
        (
            "Quarter Ping",
            &[
                ("feedback", 0.45),
                ("mix", 0.25),
                ("sync", 1.0),
                ("routing", 1.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_hp_hz", 180.0),
                ("feedback_lp_enabled", 1.0),
                ("feedback_lp_hz", 7000.0),
                ("duck", 0.35),
            ],
        ),
        (
            "Short Slap",
            &[
                ("time_ms", 90.0),
                ("feedback", 0.1),
                ("mix", 0.18),
                ("right_time_ms", 90.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_lp_enabled", 1.0),
                ("feedback_lp_hz", 10_000.0),
            ],
        ),
        (
            "Wide Eighths",
            &[
                ("feedback", 0.32),
                ("mix", 0.2),
                ("sync", 1.0),
                ("left_division", 3.0),
                ("link", 0.0),
                ("right_division", 3.0),
                ("right_modifier", 1.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_hp_hz", 150.0),
                ("feedback_lp_enabled", 1.0),
                ("feedback_lp_hz", 9000.0),
                ("duck", 0.1),
            ],
        ),
        (
            "Triplet Taps",
            &[
                ("feedback", 0.4),
                ("mix", 0.24),
                ("sync", 1.0),
                ("left_division", 3.0),
                ("left_modifier", 2.0),
                ("routing", 1.0),
                ("right_division", 3.0),
                ("right_modifier", 2.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_hp_hz", 160.0),
                ("feedback_lp_enabled", 1.0),
                ("duck", 0.15),
            ],
        ),
        (
            "Long Space",
            &[
                ("feedback", 0.65),
                ("mix", 0.18),
                ("sync", 1.0),
                ("left_division", 6.0),
                ("left_modifier", 1.0),
                ("right_division", 6.0),
                ("right_modifier", 1.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_hp_hz", 250.0),
                ("feedback_lp_enabled", 1.0),
                ("feedback_lp_hz", 4000.0),
                ("duck", 0.45),
            ],
        ),
        (
            "Wet Return",
            &[
                ("time_ms", 250.0),
                ("feedback", 0.25),
                ("mix", 1.0),
                ("right_time_ms", 250.0),
                ("feedback_hp_enabled", 1.0),
                ("feedback_hp_hz", 180.0),
                ("feedback_lp_enabled", 1.0),
                ("feedback_lp_hz", 6500.0),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Delay),
        &DELAY_KEYS,
        vibez_dsp::delay::DELAY_PARAMS,
        &presets,
    )
}

/// Sol's spaces, his decay times given as the room sizes that reach them.
fn reverb_factory_presets() -> Vec<DevicePreset> {
    let room = vibez_dsp::reverb::room_for_decay;
    let presets: [(&str, Vec<(&str, f32)>); 8] = [
        ("Init", vec![]),
        (
            "Small Space",
            vec![
                ("room_size", room(0.45)),
                ("damping", 0.25),
                ("mix", 0.16),
                ("predelay_ms", 5.0),
                ("width", 0.65),
                ("low_cut_hz", 100.0),
                ("high_cut_hz", 14_000.0),
            ],
        ),
        (
            "Drum Room",
            vec![
                ("room_size", room(0.8)),
                ("damping", 0.4),
                ("mix", 0.18),
                ("predelay_ms", 12.0),
                ("width", 0.85),
                ("diffusion", 0.75),
                ("low_cut_hz", 150.0),
                ("high_cut_hz", 11_000.0),
            ],
        ),
        (
            "Warm Hall",
            vec![
                ("room_size", room(2.4)),
                ("damping", 0.6),
                ("mix", 0.22),
                ("predelay_ms", 24.0),
                ("diffusion", 0.8),
                ("low_cut_hz", 120.0),
                ("high_cut_hz", 10_000.0),
            ],
        ),
        (
            "Clear Hall",
            vec![
                ("room_size", room(2.0)),
                ("damping", 0.2),
                ("mix", 0.2),
                ("predelay_ms", 18.0),
                ("diffusion", 0.75),
                ("low_cut_hz", 80.0),
                ("high_cut_hz", 18_000.0),
            ],
        ),
        (
            "Long Cloud",
            vec![
                ("room_size", room(8.0)),
                ("damping", 0.55),
                ("mix", 0.25),
                ("predelay_ms", 40.0),
                ("diffusion", 0.9),
                ("low_cut_hz", 180.0),
                ("high_cut_hz", 12_000.0),
            ],
        ),
        (
            "Wide Ambience",
            vec![
                ("room_size", room(1.3)),
                ("damping", 0.3),
                ("mix", 0.17),
                ("predelay_ms", 8.0),
                ("diffusion", 0.65),
                ("low_cut_hz", 160.0),
                ("high_cut_hz", 16_000.0),
            ],
        ),
        (
            "Vocal Air",
            vec![
                ("room_size", room(1.8)),
                ("damping", 0.35),
                ("mix", 0.18),
                ("predelay_sync", 5.0),
                ("width", 0.9),
                ("diffusion", 0.8),
                ("low_cut_hz", 180.0),
                ("high_cut_hz", 15_000.0),
            ],
        ),
    ];
    let presets: Vec<(&str, &[(&str, f32)])> = presets
        .iter()
        .map(|(name, changes)| (*name, changes.as_slice()))
        .collect();
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Reverb),
        &REVERB_KEYS,
        vibez_dsp::reverb::REVERB_PARAMS,
        &presets,
    )
}

fn phaser_factory_presets() -> Vec<DevicePreset> {
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        ("Slow Sweep", &[("depth", 0.6), ("stereo_phase", 90.0)]),
        (
            "Subtle Width",
            &[
                ("rate", 0.2),
                ("depth", 0.35),
                ("feedback", 0.1),
                ("mix", 0.3),
                ("stages", 0.0),
                ("stereo_phase", 120.0),
            ],
        ),
        (
            "Jet",
            &[
                ("rate", 0.15),
                ("depth", 0.9),
                ("feedback", 0.8),
                ("centre", 400.0),
                ("stages", 3.0),
            ],
        ),
        (
            "Deep Twelve",
            &[
                ("rate", 0.1),
                ("depth", 0.8),
                ("feedback", 0.5),
                ("centre", 300.0),
                ("stages", 4.0),
            ],
        ),
        (
            "Hollow",
            &[
                ("rate", 1.0),
                ("depth", 0.5),
                ("feedback", 0.7),
                ("centre", 1_200.0),
                ("stages", 2.0),
                ("polarity", 1.0),
            ],
        ),
        (
            "Quarter Swirl",
            &[
                ("depth", 0.5),
                ("feedback", 0.4),
                ("stereo_phase", 180.0),
                ("sync", 4.0),
            ],
        ),
        (
            "Wobble",
            &[
                ("rate", 4.0),
                ("depth", 0.4),
                ("feedback", 0.2),
                ("shape", 1.0),
            ],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::Phaser),
        &PHASER_KEYS,
        vibez_dsp::phaser::PHASER_PARAMS,
        &presets,
    )
}

fn auto_pan_factory_presets() -> Vec<DevicePreset> {
    use vibez_dsp::auto_pan::shape;
    let presets: [(&str, &[(&str, f32)]); 8] = [
        ("Init", &[]),
        (
            "Wide Eighths",
            &[("depth", 0.75), ("smoothing", 0.05), ("sync", 5.0)],
        ),
        ("Gentle Drift", &[("rate", 0.25), ("depth", 0.4)]),
        (
            "Slow Circle",
            &[
                ("depth", 0.6),
                ("phase", 90.0),
                ("sync", 2.0),
                ("retrigger", 1.0),
            ],
        ),
        (
            "Ping Sixteenths",
            &[
                ("depth", 1.0),
                ("shape", shape::SQUARE as f32),
                ("smoothing", 0.2),
                ("sync", 6.0),
                ("retrigger", 1.0),
            ],
        ),
        (
            "Quarter Chop",
            &[
                ("depth", 1.0),
                ("shape", shape::SQUARE as f32),
                ("mode", 1.0),
                ("phase", 0.0),
                ("smoothing", 0.1),
                ("sync", 4.0),
                ("retrigger", 1.0),
            ],
        ),
        (
            "Pump",
            &[
                ("depth", 0.8),
                ("shape", shape::SAW_DOWN as f32),
                ("mode", 1.0),
                ("phase", 0.0),
                ("offset", 0.5),
                ("smoothing", 0.1),
                ("sync", 4.0),
                ("retrigger", 1.0),
            ],
        ),
        (
            "Tremolo",
            &[("rate", 5.0), ("depth", 0.5), ("mode", 1.0), ("phase", 0.0)],
        ),
    ];
    keyed_presets(
        DevicePresetKind::Effect(EffectType::AutoPan),
        &AUTO_PAN_KEYS,
        vibez_dsp::auto_pan::AUTO_PAN_PARAMS,
        &presets,
    )
}

/// Presets that start from the device's defaults and change the named keys.
fn keyed_presets(
    kind: DevicePresetKind,
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
            DevicePreset::new(kind, *name, parameters).expect("Factory preset values are valid")
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

    fn bitcrush() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Bitcrush, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Bitcrush,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn bitcrush_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = bitcrush();
        let presets = factory_presets(EffectType::Bitcrush);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = bitcrush();
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
    fn bitcrush_switches_in_a_preset_must_be_whole() {
        let effect = bitcrush();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["rounding", "dither", "post_lp_enabled"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
    }

    fn delay() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Delay, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Delay,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn delay_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = delay();
        let presets = factory_presets(EffectType::Delay);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = delay();
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

    fn echo() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Echo, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Echo,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn echo_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = echo();
        let presets = factory_presets(EffectType::Echo);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        assert!(presets.iter().any(|preset| preset.name() == "Worn Tape"));
        for preset in &presets[1..] {
            let mut applied = echo();
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
    fn saturator_factory_presets_are_valid_distinct_and_start_from_init() {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Saturator, 44_100.0).param_descriptors();
        let saturator = || UiEffect {
            effect_type: EffectType::Saturator,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        };
        let effect = saturator();
        let presets = factory_presets(EffectType::Saturator);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        assert!(presets
            .iter()
            .any(|preset| preset.name() == "Soft Harmonics"));
        for preset in &presets[1..] {
            let mut applied = saturator();
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
    fn delay_choices_in_a_preset_must_be_whole() {
        let effect = delay();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["sync", "left_division", "routing", "link", "time_change"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
    }

    fn reverb() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Reverb, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Reverb,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn reverb_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = reverb();
        let presets = factory_presets(EffectType::Reverb);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = reverb();
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
    fn reverb_choices_in_a_preset_must_be_whole() {
        let effect = reverb();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["predelay_sync", "freeze"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
    }

    fn auto_pan() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::AutoPan, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::AutoPan,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn auto_pan_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = auto_pan();
        let presets = factory_presets(EffectType::AutoPan);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert!(presets.iter().any(|preset| preset.name() == "Wide Eighths"));
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = auto_pan();
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
    fn auto_pan_choices_in_a_preset_must_be_whole() {
        let effect = auto_pan();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["mode", "sync", "retrigger"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
    }

    fn sampler_track() -> crate::state::ProjectTrack {
        let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::Sampler);
        track.instrument_params = vibez_instruments::sampler::SAMPLER_PARAMS
            .iter()
            .map(|descriptor| descriptor.default)
            .collect();
        track
    }

    #[test]
    fn sampler_factory_presets_are_valid_distinct_and_start_from_init() {
        let track = sampler_track();
        let device = DeviceParams::instrument(&track).unwrap();
        let presets = presets_for(device.kind);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert!(presets.iter().any(|preset| preset.name() == "Warm Rhodes"));
        assert_eq!(prepare_updates(device, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = sampler_track();
            for (index, value) in prepare_updates(device, preset).unwrap() {
                applied.instrument_params[index] = value;
            }
            assert_ne!(
                applied.instrument_params,
                track.instrument_params,
                "{}",
                preset.name()
            );
            let captured = capture(DeviceParams::instrument(&applied).unwrap(), preset.name());
            assert_eq!(captured.as_ref(), Ok(preset));
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn a_sampler_preset_leaves_the_sample_s_own_points_alone() {
        use vibez_instruments::sampler::param;
        let mut track = sampler_track();
        for (index, value) in [
            (param::ROOT_NOTE, 48.0),
            (param::START, 0.2),
            (param::END, 0.8),
            (param::LOOP_START, 0.3),
            (param::LOOP_END, 0.7),
        ] {
            track.instrument_params[index] = value;
        }
        let device = DeviceParams::instrument(&track).unwrap();
        for preset in presets_for(device.kind) {
            for (index, _) in prepare_updates(device, &preset).unwrap() {
                assert!(
                    ![
                        param::ROOT_NOTE,
                        param::START,
                        param::END,
                        param::LOOP_START,
                        param::LOOP_END
                    ]
                    .contains(&index),
                    "{} moves {index}",
                    preset.name()
                );
            }
        }
        let captured = capture(device, "Current").unwrap();
        for key in ["root_note", "start", "end", "loop_start", "loop_end"] {
            assert!(!captured.parameters().contains_key(key), "{key}");
        }
        for key in ["loop", "one_shot", "reverse", "filter_type", "key_low"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(device, &fractional).is_err(), "{key}");
        }
    }

    fn synth_track() -> crate::state::ProjectTrack {
        let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::SubtractiveSynth);
        track.instrument_params = vibez_instruments::synth::SYNTH_PARAMS
            .iter()
            .map(|descriptor| descriptor.default)
            .collect();
        track
    }

    #[test]
    fn synth_factory_presets_are_valid_distinct_and_start_from_init() {
        let track = synth_track();
        let device = DeviceParams::instrument(&track).unwrap();
        let presets = presets_for(device.kind);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert!(presets.iter().any(|preset| preset.name() == "Soft Analog"));
        assert_eq!(prepare_updates(device, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = synth_track();
            for (index, value) in prepare_updates(device, preset).unwrap() {
                applied.instrument_params[index] = value;
            }
            assert_ne!(
                applied.instrument_params,
                track.instrument_params,
                "{}",
                preset.name()
            );
            let captured = capture(DeviceParams::instrument(&applied).unwrap(), preset.name());
            assert_eq!(captured.as_ref(), Ok(preset));
        }
        let mut names: Vec<_> = presets.iter().map(DevicePreset::name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "names are unique");
    }

    #[test]
    fn synth_choices_in_a_preset_must_be_whole() {
        let track = synth_track();
        let device = DeviceParams::instrument(&track).unwrap();
        let captured = capture(device, "Current").unwrap();
        for key in [
            "waveform",
            "octave",
            "filter_type",
            "voice_mode",
            "voices",
            "lfo_target",
        ] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(device, &fractional).is_err(), "{key}");
        }
    }

    fn phaser() -> UiEffect {
        let descriptors =
            vibez_dsp::factory::create_effect(EffectType::Phaser, 44_100.0).param_descriptors();
        UiEffect {
            effect_type: EffectType::Phaser,
            params: descriptors
                .iter()
                .map(|descriptor| descriptor.default)
                .collect(),
            descriptors,
            ..gain()
        }
    }

    #[test]
    fn phaser_factory_presets_are_valid_distinct_and_start_from_init() {
        let effect = phaser();
        let presets = factory_presets(EffectType::Phaser);
        assert!(presets.len() >= 6);
        assert_eq!(presets[0].name(), "Init");
        assert!(presets.iter().any(|preset| preset.name() == "Slow Sweep"));
        assert_eq!(prepare_updates(&effect, &presets[0]), Ok(Vec::new()));
        for preset in &presets[1..] {
            let mut applied = phaser();
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
    fn phaser_choices_in_a_preset_must_be_whole() {
        let effect = phaser();
        let captured = capture(&effect, "Current").unwrap();
        for key in ["stages", "polarity", "shape", "sync"] {
            let mut parameters = captured.parameters().clone();
            assert!(parameters.contains_key(key), "{key}");
            parameters.insert(key.into(), 0.5);
            let fractional = DevicePreset::new(captured.kind(), "Fractional", parameters).unwrap();
            assert!(prepare_updates(&effect, &fractional).is_err(), "{key}");
        }
    }
}
