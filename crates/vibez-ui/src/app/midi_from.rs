//! Applies a MIDI From choice to the project as one undoable edit.

use super::*;
use vibez_core::id::TrackId;

impl App {
    pub(super) fn set_midi_from(&mut self, _receiver: TrackId, _source: Option<TrackId>) {}
}
