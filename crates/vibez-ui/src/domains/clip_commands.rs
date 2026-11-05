//! Engine commands that send one whole arrangement clip, built from its UI model.

use vibez_core::id::TrackId;
use vibez_engine::clip_insertion::PreparedNoteClipInsertion;
use vibez_engine::commands::EngineCommand;

use crate::state::UiNoteClip;

impl UiNoteClip {
    /// The clip with all its notes as one insertion.
    pub fn add_command(&self, track_id: TrackId) -> EngineCommand {
        EngineCommand::AddNoteClip {
            track_id,
            insertion: PreparedNoteClipInsertion::new(
                self.id,
                self.position_beats,
                self.duration_beats,
                self.start_marker_beats,
                self.loop_enabled,
                self.loop_start_beats,
                self.loop_end_beats,
                self.groove_grid,
            )
            .with_notes(self.notes.clone()),
        }
    }
}
