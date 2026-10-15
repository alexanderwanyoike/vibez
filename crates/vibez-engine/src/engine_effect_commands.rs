//! Typed audio-thread effect commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_remove_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        allowance: &mut RetirementAllowance,
    ) {
        let removed = self.channel_mut(track_id).and_then(|track| {
            track
                .effects
                .iter()
                .position(|e| e.id == effect_id)
                .map(|pos| track.effects.remove(pos))
        });
        if let Some(slot) = removed {
            self.dispose_effect(slot.effect, allowance);
        }
    }

    pub(super) fn command_set_effect_param(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        param_index: usize,
        value: f32,
    ) {
        self.set_effect_param(track_id, effect_id, param_index, value);
    }

    pub(super) fn command_set_effect_bypass(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        bypass: bool,
    ) {
        if let Some(track) = self.channel_mut(track_id) {
            if let Some(slot) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                slot.bypass = bypass;
            }
        }
    }

    pub(super) fn command_move_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        new_index: usize,
    ) {
        if let Some(track) = self.channel_mut(track_id) {
            if let Some(old_idx) = track.effects.iter().position(|e| e.id == effect_id) {
                let slot = track.effects.remove(old_idx);
                let idx = new_index.min(track.effects.len());
                track.effects.insert(idx, slot);
            }
        }
    }
}
