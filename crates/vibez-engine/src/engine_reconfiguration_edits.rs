//! Keeps a stopped owner tied to its device lifetime while commands continue.

use super::*;

impl AudioEngine {
    pub(super) fn prepare_device_edit(&mut self, command: &mut EngineCommand) {
        let Some(mut pending) = self.pending_device_reconfiguration else {
            return;
        };
        let mut invalid = false;
        match command {
            EngineCommand::UnloadAudio => invalid = true,
            EngineCommand::RemoveTrack(track)
            | EngineCommand::RemoveBus(track)
            | EngineCommand::AddTrack(track, _)
            | EngineCommand::AddBus(track, _)
            | EngineCommand::AddMidiTrack(track, _)
            | EngineCommand::AddInstrumentTrack(track, _, _) => {
                invalid = *track == pending.track_id;
            }
            EngineCommand::SetPluginInstrument { track_id, .. }
            | EngineCommand::SetTrackInstrument(track_id, ..)
            | EngineCommand::RemoveTrackInstrument(track_id) => {
                invalid = *track_id == pending.track_id && pending.effect_id.is_none();
            }
            EngineCommand::RemoveEffect(track, effect) if *track == pending.track_id => {
                invalid = pending.effect_id == Some(*effect);
                if !invalid {
                    if let Some(index) = self
                        .channel_mut(*track)
                        .and_then(|track| track.effects.iter().position(|slot| slot.id == *effect))
                    {
                        if index < pending.position {
                            pending.position -= 1;
                        }
                    }
                }
            }
            EngineCommand::AddEffect {
                track_id,
                effect_id,
                position,
                ..
            }
            | EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                position,
                ..
            } if *track_id == pending.track_id && pending.effect_id.is_some() => {
                invalid = pending.effect_id == Some(*effect_id);
                if !invalid {
                    if let Some(index) = position {
                        if *index <= pending.position {
                            pending.position += 1;
                        } else {
                            *index -= 1;
                        }
                    }
                }
            }
            EngineCommand::SetEffectBypass {
                track_id,
                effect_id,
                bypass,
            } if *track_id == pending.track_id && pending.effect_id == Some(*effect_id) => {
                pending.bypass = *bypass;
            }
            EngineCommand::MoveEffect {
                track_id,
                effect_id,
                new_index,
            } if *track_id == pending.track_id && pending.effect_id.is_some() => {
                if pending.effect_id == Some(*effect_id) {
                    pending.position = *new_index;
                } else if let Some(index) = self
                    .channel_mut(*track_id)
                    .and_then(|track| track.effects.iter().position(|slot| slot.id == *effect_id))
                {
                    let old = index + usize::from(index >= pending.position);
                    if old < pending.position && *new_index >= pending.position {
                        pending.position -= 1;
                    } else if old > pending.position && *new_index <= pending.position {
                        pending.position += 1;
                    }
                    // UI indices include the stopped slot that is currently
                    // owned on main, unlike the compact resident vector.
                    *new_index =
                        new_index.saturating_sub(usize::from(*new_index > pending.position));
                }
            }
            _ => {}
        }
        if invalid {
            self.pending_device_reconfiguration = None;
            self.compensation_suspended = false;
        } else {
            self.pending_device_reconfiguration = Some(pending);
        }
    }
}
