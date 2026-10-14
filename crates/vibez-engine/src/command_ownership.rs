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
            Self::RemoveTrack(id) | Self::RemoveBus(id) => DeviceOwnerInvalidation::Channel(*id),
            Self::AddTrack(channel) | Self::AddBus(channel) => {
                DeviceOwnerInvalidation::Channel(channel.id)
            }
            Self::RemoveEffect(track, effect)
            | Self::InsertEffect {
                track_id: track,
                effect_id: effect,
                ..
            } => DeviceOwnerInvalidation::Effect(*track, *effect),
            Self::SetInstrument { track_id, .. } | Self::RemoveTrackInstrument(track_id) => {
                DeviceOwnerInvalidation::Instrument(*track_id)
            }
            _ => DeviceOwnerInvalidation::None,
        }
    }
    /// Edits that leave a published routing graph stale until the next
    /// `SetRouting`; the engine mutes the project in between.
    pub fn edits_effect_chain(&self) -> bool {
        matches!(
            self,
            Self::RemoveEffect(..) | Self::MoveEffect { .. } | Self::InsertEffect { .. }
        )
    }
    pub(crate) fn retirement_reserve(&self) -> usize {
        match self {
            // Failed restoration can return reserved storage, the device,
            // and the owned rejection cause in the same command.
            Self::RejectDeviceReconfiguration { .. } => 3,
            Self::ResumeDeviceReconfiguration { .. } => 2,
            Self::ReplaceClipAudio { .. } | Self::ReplaceClipBuffer { .. } => 1,
            Self::SetClipWarpMarkers { .. } => 1,
            Self::ReorderTracks(_) => 1,
            Self::LoadSamplerSample { .. } => 1,
            _ => 2,
        }
    }
}
