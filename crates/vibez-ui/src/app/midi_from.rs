//! Applies a MIDI From choice to the project as one undoable edit.

use super::*;
use vibez_core::id::TrackId;

impl App {
    pub(super) fn set_midi_from(&mut self, receiver: TrackId, source: Option<TrackId>) {
        let tracks = &self.state.project_tracks;
        let Some(next) = crate::domains::midi_from::change(tracks, receiver, source) else {
            return;
        };
        self.push_undo_snapshot(None);
        crate::domains::midi_from::apply(
            Arc::make_mut(&mut self.state.project_tracks),
            receiver,
            next,
        );
        self.mark_project_dirty();
    }
}
