//! Applies a MIDI From choice to the project as one undoable edit.

use super::*;
use vibez_core::id::TrackId;

impl App {
    pub(super) fn set_midi_from(&mut self, receiver: TrackId, source: Option<TrackId>) {
        let tracks = &self.state.project_tracks;
        if crate::domains::midi_from::change(tracks, receiver, source).is_none() {
            return;
        }
        self.push_undo_snapshot(None);
        crate::domains::midi_from::set(
            Arc::make_mut(&mut self.state.project_tracks),
            receiver,
            source,
        );
        self.mark_project_dirty();
    }
}
