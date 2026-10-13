//! Typed audio-thread device commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_add_plugin_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        effect: Box<dyn vibez_dsp::effect::AudioEffect>,
        position: Option<usize>,
    ) {
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

    pub(super) fn command_audition_note(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pitch: u8,
        velocity: u8,
        on: bool,
    ) {
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

    pub(super) fn command_set_plugin_instrument(
        &mut self,
        track_id: vibez_core::id::TrackId,
        instrument: Box<dyn vibez_instruments::Instrument>,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(old) = track.instrument.replace(instrument) {
                self.dispose_instrument(old);
            }
        }
    }
}
