use super::*;

impl AudioEngine {
    pub(super) fn apply_effect_command(&mut self, command: EngineCommand) {
        match command {
            // -- Effects --
            EngineCommand::AddEffect {
                track_id,
                effect_id,
                effect_type,
                position,
            } => {
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
            EngineCommand::RemoveEffect(track_id, effect_id) => {
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
            EngineCommand::SetEffectParam {
                track_id,
                effect_id,
                param_index,
                value,
            } => {
                self.set_effect_param(track_id, effect_id, param_index, value);
            }
            EngineCommand::SetEffectBypass {
                track_id,
                effect_id,
                bypass,
            } => {
                if let Some(track) = self.channel_mut(track_id) {
                    if let Some(slot) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        slot.bypass = bypass;
                    }
                }
            }
            EngineCommand::MoveEffect {
                track_id,
                effect_id,
                new_index,
            } => {
                if let Some(track) = self.channel_mut(track_id) {
                    if let Some(old_idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        let slot = track.effects.remove(old_idx);
                        let idx = new_index.min(track.effects.len());
                        track.effects.insert(idx, slot);
                    }
                }
            }

            _ => unreachable!("effect command dispatch"),
        }
    }
}
