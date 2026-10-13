//! Keeps a stopped owner tied to its device lifetime while commands continue.

use super::*;

impl AudioEngine {
    pub(super) fn retain_device_parameter(&mut self, command: &EngineCommand) -> bool {
        let Some(pending) = self.pending_device_reconfiguration else {
            return false;
        };
        let parameter = match command {
            EngineCommand::SetEffectParam {
                track_id,
                effect_id,
                param_index,
                value,
            } if *track_id == pending.track_id && Some(*effect_id) == pending.effect_id => {
                Some((*param_index, *value))
            }
            EngineCommand::SetInstrumentParam {
                track_id,
                param_index,
                value,
            } if *track_id == pending.track_id && pending.effect_id.is_none() => {
                Some((*param_index, *value))
            }
            _ => None,
        };
        let Some((index, value)) = parameter else {
            return false;
        };
        if let Some(previous) = self
            .pending_device_parameters
            .iter_mut()
            .find(|(id, _)| *id == index)
        {
            previous.1 = value;
        } else if self.pending_device_parameters.len() < self.pending_device_parameters.capacity() {
            self.pending_device_parameters.push((index, value));
        } else {
            // Match the hosted parameter event bound without blocking the
            // returning owner behind its own queued edits.
            self.report_compensation_failure(
                pending.track_id,
                pending.effect_id,
                "Device recovery parameter queue is full; parameter edit was rejected",
            );
        }
        true
    }

    pub(super) fn apply_pending_device_parameters(
        &mut self,
        track: TrackId,
        effect: Option<vibez_core::id::EffectId>,
        mut set: impl FnMut(usize, f32) -> bool,
    ) {
        let mut rejected = false;
        for &(index, value) in &self.pending_device_parameters {
            rejected |= !set(index, value);
        }
        self.pending_device_parameters.clear();
        if rejected {
            self.report_compensation_failure(
                track,
                effect,
                "Recovered device rejected a retained parameter edit",
            );
        }
    }

    pub(super) fn prepare_device_edit(&mut self, command: &mut EngineCommand) {
        let Some(mut pending) = self.pending_device_reconfiguration else {
            return;
        };
        let invalid = command
            .device_owner_invalidation()
            .matches(pending.track_id, pending.effect_id);
        match command {
            EngineCommand::RemoveEffect(track, effect) if *track == pending.track_id => {
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
            self.pending_device_parameters.clear();
            self.compensation_suspended = false;
        } else {
            self.pending_device_reconfiguration = Some(pending);
        }
    }
}
