//! Owns native Gate page selection independently of persisted sound and routing.

use super::gate_history::GateHistory;
use crate::state::UiEffect;
use vibez_core::{
    effect::EffectType,
    id::{EffectId, TrackId},
    routing::ExternalInputId,
};

pub fn is_native_gate(effect: &UiEffect) -> bool {
    effect.effect_type == EffectType::Gate
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GatePage {
    Sound,
    #[default]
    Dynamics,
    Detector,
    Sidechain,
    Metering,
}
impl GatePage {
    pub const ALL: [Self; 5] = [
        Self::Sound,
        Self::Dynamics,
        Self::Detector,
        Self::Sidechain,
        Self::Metering,
    ];
}
impl std::fmt::Display for GatePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sound => "Sound",
            Self::Dynamics => "Dynamics",
            Self::Detector => "Detector",
            Self::Sidechain => "Sidechain",
            Self::Metering => "Metering",
        })
    }
}

#[derive(Clone, Debug)]
pub enum GateMsg {
    Page(TrackId, EffectId, GatePage),
    SidechainInput(TrackId, EffectId, ExternalInputId),
}
impl GateMsg {
    pub fn destination(&self) -> (TrackId, EffectId) {
        match self {
            Self::Page(track, effect, _) | Self::SidechainInput(track, effect, _) => {
                (*track, *effect)
            }
        }
    }
}

pub struct GatePanel {
    pub page: GatePage,
    pub sidechain_input: ExternalInputId,
    pub history: GateHistory,
}
impl Default for GatePanel {
    fn default() -> Self {
        Self {
            page: GatePage::default(),
            sidechain_input: ExternalInputId(0),
            history: GateHistory::default(),
        }
    }
}
impl GatePanel {
    pub fn update(&mut self, message: GateMsg) {
        match message {
            GateMsg::Page(_, _, page) => self.page = page,
            GateMsg::SidechainInput(_, _, input) => self.sidechain_input = input,
        }
    }
}
