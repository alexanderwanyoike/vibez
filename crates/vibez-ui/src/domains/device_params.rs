//! A native device's parameters wherever it sits on a track: one of its
//! effects, or its instrument. Shared controls, typed values and presets
//! read and change values through this, so they serve both alike.

use vibez_core::device_preset::DevicePresetKind;
use vibez_core::effect::ParamDescriptor;
use vibez_core::id::{EffectId, TrackId};

use crate::domains::devices::DevicesMsg;
use crate::message::Message;
use crate::state::{UiEffect, UiTrack};

/// Which device on a track a control belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceSlot {
    Effect(EffectId),
    Instrument,
    /// A Drum Rack pad's Sampler.
    DrumPad(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceParams<'a> {
    pub slot: DeviceSlot,
    pub kind: DevicePresetKind,
    pub params: &'a [f32],
    pub descriptors: &'static [ParamDescriptor],
    pub bypass: bool,
    /// Plugins keep their own presets and value formats.
    pub plugin: bool,
}

impl<'a> From<&'a UiEffect> for DeviceParams<'a> {
    fn from(effect: &'a UiEffect) -> Self {
        Self {
            slot: DeviceSlot::Effect(effect.id),
            kind: DevicePresetKind::Effect(effect.effect_type),
            params: &effect.params,
            descriptors: effect.descriptors,
            bypass: effect.bypass,
            plugin: effect.plugin_ref.is_some() || effect.plugin_name.is_some(),
        }
    }
}

impl<'a> DeviceParams<'a> {
    /// The track's built-in instrument, if it has one.
    pub fn instrument(track: &'a UiTrack) -> Option<Self> {
        let kind = track.instrument_kind.filter(|_| track.has_instrument)?;
        if track.plugin_instrument_ref.is_some() || track.plugin_instrument_name.is_some() {
            return None;
        }
        Some(Self {
            slot: DeviceSlot::Instrument,
            kind: DevicePresetKind::Instrument(kind),
            params: &track.instrument_params,
            descriptors: vibez_instruments::descriptors_for(kind),
            bypass: false,
            plugin: false,
        })
    }

    /// The device in `slot` on `track`.
    pub fn find(track: &'a UiTrack, slot: DeviceSlot) -> Option<Self> {
        match slot {
            DeviceSlot::Effect(id) => track
                .effects
                .iter()
                .find(|effect| effect.id == id)
                .map(Self::from),
            DeviceSlot::Instrument => Self::instrument(track),
            DeviceSlot::DrumPad(pad) => track.drum_rack_pads.get(pad).map(|pad| Self {
                slot,
                kind: DevicePresetKind::Instrument(vibez_core::midi::InstrumentKind::Sampler),
                params: &pad.params,
                descriptors: vibez_instruments::sampler::SAMPLER_PARAMS,
                bypass: false,
                plugin: false,
            }),
        }
    }

    /// The stored value, or the default for one not stored yet.
    pub fn value(&self, index: usize) -> f32 {
        self.params
            .get(index)
            .copied()
            .or_else(|| self.descriptors.get(index).map(|d| d.default))
            .unwrap_or(0.0)
    }
}

impl From<EffectId> for DeviceSlot {
    fn from(id: EffectId) -> Self {
        Self::Effect(id)
    }
}

impl DeviceSlot {
    /// Sets one value on this device.
    pub fn set(self, track_id: TrackId, index: usize, value: f32) -> Message {
        match self {
            Self::Effect(id) => Message::set_effect_param(track_id, id, index, value),
            Self::Instrument => Message::set_instrument_param(track_id, index, value),
            Self::DrumPad(pad) => Message::Devices(DevicesMsg::SetDrumPadParams(
                track_id,
                pad,
                vec![(index, value)],
            )),
        }
    }

    /// Sets several values on this device as one edit.
    pub fn set_all(self, track_id: TrackId, updates: Vec<(usize, f32)>) -> DevicesMsg {
        match self {
            Self::Effect(id) => DevicesMsg::SetEffectParams(track_id, id, updates),
            Self::Instrument => DevicesMsg::SetInstrumentParams(track_id, updates),
            Self::DrumPad(pad) => DevicesMsg::SetDrumPadParams(track_id, pad, updates),
        }
    }
}
