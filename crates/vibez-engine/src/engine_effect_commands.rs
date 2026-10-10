//! Typed audio-thread effect commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_add_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        effect_type: vibez_core::effect::EffectType,
        position: Option<usize>,
    ) {
        let effect = create_effect(effect_type, self.sample_rate as f32);
        if let Some(track) = self.channel_mut(track_id) {
            let slot = EffectSlot {
                id: effect_id,
                effect,
                bypass: false,
            };
            if let Some(pos) = position {
                let idx = pos.min(track.effects.len());
                track.effects.insert(idx, slot);
            } else {
                track.effects.push(slot);
            }
        }
    }

    pub(super) fn command_remove_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
    ) {
        let removed = self.channel_mut(track_id).and_then(|track| {
            track
                .effects
                .iter()
                .position(|e| e.id == effect_id)
                .map(|pos| track.effects.remove(pos))
        });
        if let Some(slot) = removed {
            self.dispose_effect(slot.effect);
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
