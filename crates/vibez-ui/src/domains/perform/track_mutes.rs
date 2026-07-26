//! Live Track Mute interaction state mirrored from engine-owned boundaries.

use super::*;

/// A semantic mute request resolved by Perform against a stable pad slot.
/// The router applies it to the single shared Project Track state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackMuteRequest {
    pub track_id: TrackId,
    pub muted: bool,
    pub quantization: TrackMuteQuantization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTrackMute {
    pub muted: bool,
    pub effective_at_samples: u64,
}

impl PerformState {
    pub const fn track_mute_quantization(&self) -> TrackMuteQuantization {
        self.track_mute_quantization
    }

    pub fn set_track_mute_quantization(&mut self, quantization: TrackMuteQuantization) {
        self.track_mute_quantization = quantization;
    }

    pub fn queue_track_mute_ui(
        &mut self,
        track_id: TrackId,
        muted: bool,
        effective_at_samples: u64,
    ) {
        self.pending_track_mutes.insert(
            track_id,
            PendingTrackMute {
                muted,
                effective_at_samples,
            },
        );
    }

    pub fn cancel_track_mute_ui(&mut self, track_id: TrackId) {
        self.pending_track_mutes.remove(&track_id);
    }

    pub fn pending_track_mute(&self, track_id: TrackId) -> Option<PendingTrackMute> {
        self.pending_track_mutes.get(&track_id).copied()
    }

    pub fn take_pending_track_mute(&mut self, track_id: TrackId) -> Option<PendingTrackMute> {
        self.pending_track_mutes.remove(&track_id)
    }

    pub fn clear_pending_track_mutes(&mut self) {
        self.pending_track_mutes.clear();
    }
}
