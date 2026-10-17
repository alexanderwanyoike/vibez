//! Typed audio-thread device commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_insert_effect(
        &mut self,
        track_id: vibez_core::id::TrackId,
        effect_id: vibez_core::id::EffectId,
        effect: Box<dyn vibez_dsp::effect::AudioEffect>,
        position: Option<usize>,
        allowance: &mut RetirementAllowance,
    ) {
        let Some(track) = self
            .channel_mut(track_id)
            .filter(|track| track.effects.len() < track.effects.capacity())
        else {
            debug_assert!(
                self.channel_mut(track_id).is_none(),
                "the UI keeps device chains within DEVICE_CHAIN_CAPACITY"
            );
            self.dispose_effect(effect, allowance);
            return;
        };
        let slot = EffectSlot {
            levels: Default::default(),
            id: effect_id,
            effect,
            bypass: false,
        };
        match position {
            Some(pos) => track.effects.insert(pos.min(track.effects.len()), slot),
            None => track.effects.push(slot),
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

    pub(super) fn command_set_instrument(
        &mut self,
        track_id: vibez_core::id::TrackId,
        instrument: Box<dyn vibez_instruments::Instrument>,
        allowance: &mut RetirementAllowance,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(old) = track.instrument.replace(instrument) {
                self.dispose_instrument(old, allowance);
            }
        } else {
            self.dispose_instrument(instrument, allowance);
        }
    }
}
