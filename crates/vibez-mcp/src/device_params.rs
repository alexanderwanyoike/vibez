//! Resolves named device parameters against Vibez's own descriptors so agents never handle raw indices.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::midi::InstrumentKind;

/// A native Vibez device an agent can place on a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    SubtractiveSynth,
    WavetableSynth,
    FmSynth,
    Sampler,
    Gain,
    Filter,
    Delay,
    Echo,
    Reverb,
    Drive,
    Saturator,
    Bitcrush,
    Compressor,
    Limiter,
    AutoPan,
    Gate,
    Phaser,
    Eq,
}

impl Device {
    pub fn instrument_kind(self) -> Option<InstrumentKind> {
        match self {
            Device::SubtractiveSynth => Some(InstrumentKind::SubtractiveSynth),
            Device::WavetableSynth => Some(InstrumentKind::WavetableSynth),
            Device::FmSynth => Some(InstrumentKind::FmSynth),
            Device::Sampler => Some(InstrumentKind::Sampler),
            _ => None,
        }
    }

    pub fn effect_type(self) -> Option<EffectType> {
        match self {
            Device::Gain => Some(EffectType::Gain),
            Device::Filter => Some(EffectType::Filter),
            Device::Delay => Some(EffectType::Delay),
            Device::Echo => Some(EffectType::Echo),
            Device::Reverb => Some(EffectType::Reverb),
            Device::Drive => Some(EffectType::Drive),
            Device::Saturator => Some(EffectType::Saturator),
            Device::Bitcrush => Some(EffectType::Bitcrush),
            Device::Compressor => Some(EffectType::Compressor),
            Device::Limiter => Some(EffectType::Limiter),
            Device::AutoPan => Some(EffectType::AutoPan),
            Device::Gate => Some(EffectType::Gate),
            Device::Phaser => Some(EffectType::Phaser),
            Device::Eq => Some(EffectType::Eq),
            _ => None,
        }
    }

    pub fn descriptors(self) -> &'static [ParamDescriptor] {
        if let Some(kind) = self.instrument_kind() {
            return vibez_instruments::descriptors_for(kind);
        }
        let effect_type = self
            .effect_type()
            .expect("every Device is an instrument or an effect");
        // Descriptors are static per effect type; the sample rate only sizes
        // the throwaway instance's buffers.
        vibez_dsp::factory::create_effect(effect_type, 48_000.0).param_descriptors()
    }

    pub fn default_params(self) -> Vec<f32> {
        self.descriptors().iter().map(|d| d.default).collect()
    }
}

/// One parameter as an agent sees it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct ParameterInfo {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: String,
}

/// The value a requested parameter actually received after clamping.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct AppliedParameter {
    pub name: String,
    pub requested: f32,
    pub applied: f32,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DeviceParamError {
    #[error("{device:?} has no parameter named \"{name}\"; known parameters: {known}")]
    UnknownParameter {
        device: Device,
        name: String,
        known: String,
    },
    #[error("parameter \"{name}\" must be a finite number")]
    NotFinite { name: String },
}

pub fn parameter_info(device: Device) -> Vec<ParameterInfo> {
    device
        .descriptors()
        .iter()
        .map(|d| ParameterInfo {
            name: d.name.to_string(),
            min: d.min,
            max: d.max,
            default: d.default,
            unit: d.unit.to_string(),
        })
        .collect()
}

/// Writes named values into `params`, clamping each to its descriptor range.
/// Names match case-insensitively; nothing is written unless every name resolves.
pub fn apply_named(
    device: Device,
    params: &mut Vec<f32>,
    values: &BTreeMap<String, f32>,
) -> Result<Vec<AppliedParameter>, DeviceParamError> {
    let descriptors = device.descriptors();
    let mut resolved = Vec::with_capacity(values.len());
    for (name, &requested) in values {
        if !requested.is_finite() {
            return Err(DeviceParamError::NotFinite { name: name.clone() });
        }
        let index = descriptors
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| DeviceParamError::UnknownParameter {
                device,
                name: name.clone(),
                known: descriptors
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", "),
            })?;
        resolved.push((index, requested));
    }

    if params.len() < descriptors.len() {
        let defaults = device.default_params();
        params.extend_from_slice(&defaults[params.len()..]);
    }
    Ok(resolved
        .into_iter()
        .map(|(index, requested)| {
            let descriptor = &descriptors[index];
            let applied = requested.clamp(descriptor.min, descriptor.max);
            params[index] = applied;
            AppliedParameter {
                name: descriptor.name.to_string(),
                requested,
                applied,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, f32)]) -> BTreeMap<String, f32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn every_device_maps_to_exactly_one_native_kind() {
        for device in ALL {
            assert!(device.instrument_kind().is_some() != device.effect_type().is_some());
        }
    }

    #[test]
    fn defaults_follow_vibez_descriptors() {
        let reverb = Device::Reverb.default_params();
        assert_eq!(reverb.len(), Device::Reverb.descriptors().len());
        assert_eq!(reverb[0], vibez_dsp::reverb::REVERB_PARAMS[0].default);
        let fm = Device::FmSynth.default_params();
        assert_eq!(fm.len(), vibez_instruments::fm::FM_PARAMS.len());
    }

    #[test]
    fn named_values_are_written_case_insensitively_and_clamped() {
        let mut params = Device::Reverb.default_params();
        let applied = apply_named(
            Device::Reverb,
            &mut params,
            &values(&[("room size", 0.8), ("Mix", 7.0)]),
        )
        .unwrap();
        assert_eq!(params[0], 0.8);
        assert_eq!(params[2], 1.0);
        let mix = applied.iter().find(|a| a.name == "Mix").unwrap();
        assert_eq!((mix.requested, mix.applied), (7.0, 1.0));
    }

    #[test]
    fn an_unknown_name_writes_nothing() {
        let mut params = Device::Delay.default_params();
        let before = params.clone();
        let error = apply_named(
            Device::Delay,
            &mut params,
            &values(&[("Time", 300.0), ("Wobble", 1.0)]),
        )
        .unwrap_err();
        assert!(matches!(error, DeviceParamError::UnknownParameter { .. }));
        assert_eq!(params, before);
    }

    #[test]
    fn short_parameter_lists_are_padded_with_defaults() {
        let mut params = vec![1000.0];
        apply_named(Device::Delay, &mut params, &values(&[("Mix", 0.2)])).unwrap();
        assert_eq!(params.len(), Device::Delay.descriptors().len());
        assert_eq!(params[0], 1000.0);
    }

    #[test]
    fn parameter_info_lists_names_and_ranges() {
        let info = parameter_info(Device::Filter);
        assert_eq!(info[0].name, "Cutoff");
        assert!(info[0].max > info[0].min);
    }

    const ALL: [Device; 18] = [
        Device::SubtractiveSynth,
        Device::WavetableSynth,
        Device::FmSynth,
        Device::Sampler,
        Device::Gain,
        Device::Filter,
        Device::Delay,
        Device::Echo,
        Device::Reverb,
        Device::Drive,
        Device::Saturator,
        Device::Bitcrush,
        Device::Compressor,
        Device::Limiter,
        Device::AutoPan,
        Device::Gate,
        Device::Phaser,
        Device::Eq,
    ];
}
