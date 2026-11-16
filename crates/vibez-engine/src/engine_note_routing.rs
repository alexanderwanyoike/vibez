//! Notes routed into a track from outside its clips: MIDI From, where a receiving track takes the notes its source track's instrument played this block, and the Browser's MIDI audition.

use super::*;
use crate::routing::ChannelIndex;

impl AudioEngine {
    pub(super) fn forget_played_notes(&mut self, channel_index: usize) {
        if let ChannelIndex::Track(track) = self.routing.channels[channel_index].binding {
            self.tracks[track].forget_played_notes();
        }
    }

    /// A receiver that skips rendering misses its source's note-offs, so it
    /// ends what it holds when it next renders.
    pub(super) fn release_routed_notes(&mut self, channel_index: usize) {
        if let ChannelIndex::Track(track) = self.routing.channels[channel_index].binding {
            self.tracks[track].routed_notes.release();
            self.tracks[track].audition_notes.release();
        }
    }

    pub(super) fn admit_audition_notes(&mut self, channel_index: usize, frames: u32) {
        if let ChannelIndex::Track(track) = self.routing.channels[channel_index].binding {
            self.audition
                .midi
                .admit_into(&mut self.tracks[track], frames);
        }
    }

    /// Works out the Browser MIDI audition's notes for this block, before
    /// any track renders.
    pub(super) fn sequence_midi_audition(&mut self, frames: usize) {
        let bpm = self.transport.bpm();
        let samples_per_beat = if bpm > 0.0 {
            f64::from(self.sample_rate) * 60.0 / bpm
        } else {
            0.0
        };
        let tracks = &self.tracks;
        self.audition.midi.sequence(
            frames,
            self.transport.is_playing(),
            samples_per_beat,
            |id| {
                tracks
                    .iter()
                    .any(|track| track.id == id && track.instrument.is_some())
            },
            &mut self.event_tx,
        );
    }

    /// The routing order renders a source before its receiver, so the
    /// source's notes for this block are already kept.
    pub(super) fn admit_routed_notes(&mut self, channel_index: usize, frames: u32) {
        let channels = &self.routing.channels;
        let ChannelIndex::Track(receiver) = channels[channel_index].binding else {
            return;
        };
        let source =
            channels[channel_index]
                .note_source
                .and_then(|source| match channels[source].binding {
                    ChannelIndex::Track(source) if source != receiver => Some(source),
                    _ => None,
                });
        let Some(source) = source else {
            self.tracks[receiver].routed_notes.admit(None, &[], frames);
            return;
        };
        let (receiving, playing) = if receiver < source {
            let (front, back) = self.tracks.split_at_mut(source);
            (&mut front[receiver], &back[0])
        } else {
            let (front, back) = self.tracks.split_at_mut(receiver);
            (&mut back[0], &front[source])
        };
        let (source, notes) = playing.played_note_source();
        receiving.routed_notes.admit(source, notes, frames);
    }

    /// Catches stops the engine makes itself (song end, faults, unload),
    /// which no Stop command announces.
    pub(super) fn release_routed_notes_after_stop(&mut self) {
        let playing = self.transport.is_playing();
        if std::mem::replace(&mut self.transport_was_playing, playing) && !playing {
            for track in &mut self.tracks {
                track.routed_notes.release();
            }
        }
    }
}
