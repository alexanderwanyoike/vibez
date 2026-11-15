//! MIDI From in the engine: each receiving track takes the notes its source track's instrument played this block.

use super::*;
use crate::routing::{ChannelIndex, PreparedRouting};

impl AudioEngine {
    /// A track that skips rendering this block must not pass on last block's notes.
    pub(super) fn forget_played_notes(&mut self, channel_index: usize) {
        if let ChannelIndex::Track(track) = self.routing.channels[channel_index].binding {
            self.tracks[track].played_notes.clear();
        }
    }

    /// The routing order renders a source before its receiver, so the
    /// source's notes for this block are already kept.
    pub(super) fn admit_routed_notes(&mut self, channel_index: usize) {
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
            self.tracks[receiver].routed_notes.admit(None, &[], 0);
            return;
        };
        let (receiving, playing) = if receiver < source {
            let (front, back) = self.tracks.split_at_mut(source);
            (&mut front[receiver], &back[0])
        } else {
            let (front, back) = self.tracks.split_at_mut(receiver);
            (&mut back[0], &front[source])
        };
        receiving
            .routed_notes
            .admit(None, playing.played_notes.notes(), 0);
    }

    /// Ends routed notes on every track whose MIDI From source `next` changes.
    pub(super) fn release_rerouted_notes(&mut self, next: &PreparedRouting) {
        for channel in &next.channels {
            let source = |routing: &PreparedRouting, channel: &crate::routing::PreparedChannel| {
                channel
                    .note_source
                    .map(|source| routing.channels[source].id)
            };
            let previous = self
                .routing
                .channel_indices
                .get(&channel.id)
                .and_then(|&index| source(&self.routing, &self.routing.channels[index]));
            if previous != source(next, channel) {
                if let Some(track) = self.tracks.iter_mut().find(|track| track.id == channel.id) {
                    track.routed_notes.release();
                }
            }
        }
    }
}
