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
        todo!()
    }

    pub fn effect_type(self) -> Option<EffectType> {
        todo!()
    }

    pub fn descriptors(self) -> &'static [ParamDescriptor] {
        todo!()
    }

    pub fn default_params(self) -> Vec<f32> {
        todo!()
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
    let _ = device;
    todo!()
}

/// Writes named values into `params`, clamping each to its descriptor range.
/// Names match case-insensitively; nothing is written unless every name resolves.
pub fn apply_named(
    device: Device,
    params: &mut Vec<f32>,
    values: &BTreeMap<String, f32>,
) -> Result<Vec<AppliedParameter>, DeviceParamError> {
    let _ = (device, params, values);
    todo!()
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
