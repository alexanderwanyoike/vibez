//! One lifetime predicate and retirement budget for UI and native command ingress.

use crate::commands::EngineCommand;
use vibez_core::id::{EffectId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceOwnerInvalidation {
    None,
    All,
    Channel(TrackId),
    Effect(TrackId, EffectId),
    Instrument(TrackId),
}
impl DeviceOwnerInvalidation {
    pub fn matches(self, track: TrackId, effect: Option<EffectId>) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Channel(id) => id == track,
            Self::Effect(id, slot) => id == track && effect == Some(slot),
            Self::Instrument(id) => id == track && effect.is_none(),
        }
    }
}
impl EngineCommand {
    pub fn device_owner_invalidation(&self) -> DeviceOwnerInvalidation {
        match self {
            Self::UnloadAudio => DeviceOwnerInvalidation::All,
            Self::RemoveTrack(id)
            | Self::RemoveBus(id)
            | Self::AddTrack(id, _)
            | Self::AddBus(id, _)
            | Self::AddMidiTrack(id, _)
            | Self::AddInstrumentTrack(id, _, _) => DeviceOwnerInvalidation::Channel(*id),
            Self::RemoveEffect(track, effect)
            | Self::AddEffect {
                track_id: track,
                effect_id: effect,
                ..
            }
            | Self::AddPluginEffect {
                track_id: track,
                effect_id: effect,
                ..
            } => DeviceOwnerInvalidation::Effect(*track, *effect),
            Self::SetPluginInstrument { track_id, .. }
            | Self::SetTrackInstrument(track_id, ..)
            | Self::RemoveTrackInstrument(track_id) => {
                DeviceOwnerInvalidation::Instrument(*track_id)
            }
            _ => DeviceOwnerInvalidation::None,
        }
    }
    pub(crate) fn retirement_reserve(&self) -> usize {
        match self {
            // Failed restoration can return reserved storage, the device,
            // and the owned rejection cause in the same command.
            Self::RejectDeviceReconfiguration { .. } => 3,
            Self::ResumeDeviceReconfiguration { .. } => 2,
            _ => 2,
        }
    }
}
