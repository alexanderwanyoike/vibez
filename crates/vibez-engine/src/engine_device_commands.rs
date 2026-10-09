use super::*;

impl AudioEngine {
    // Returning the inline owner avoids allocating a command box in the callback.
    #[allow(clippy::result_large_err)]
    pub(super) fn handle_device_command(
        &mut self,
        cmd: EngineCommand,
    ) -> Result<(), EngineCommand> {
        match cmd {
            // -- Effects --
            EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                effect,
                position,
            } => {
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
            EngineCommand::AuditionNote {
                track_id,
                pitch,
                velocity,
                on,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        if on {
                            instrument.note_on(pitch, velocity);
                        } else {
                            instrument.note_off(pitch);
                        }
                    }
                }
            }
            EngineCommand::SetPluginInstrument {
                track_id,
                instrument,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(old) = track.instrument.replace(instrument) {
                        self.dispose_instrument(old);
                    }
                }
            }
            other => return Err(other),
        }
        Ok(())
    }
}
