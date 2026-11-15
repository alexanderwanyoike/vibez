//! MIDI From in the engine: each receiving track takes the notes its source track's instrument played this block.

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
        }
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

    pub(super) fn release_routed_notes_after_stop(&mut self) {
        let playing = self.transport.is_playing();
        if std::mem::replace(&mut self.routed_notes_playing, playing) && !playing {
            for track in &mut self.tracks {
                track.routed_notes.release();
            }
        }
    }
}
