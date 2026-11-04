//! Engine commands that send one whole arrangement clip, built from its UI model.

use vibez_core::id::TrackId;
use vibez_engine::clip_insertion::{PreparedClipInsertion, PreparedNoteClipInsertion};
use vibez_engine::commands::EngineCommand;

use crate::state::{UiClip, UiNoteClip};

impl UiClip {
    /// The clip with its audio as one insertion.
    pub fn add_command(&self, track_id: TrackId) -> EngineCommand {
        self.add_command_beside(track_id, 0)
    }

    /// See [`UiNoteClip::add_command_beside`].
    pub fn add_command_beside(&self, track_id: TrackId, resident: usize) -> EngineCommand {
        EngineCommand::AddClip {
            insertion: PreparedClipInsertion::with_capacity(resident + 1),
            track_id,
            clip_id: self.id,
            audio: std::sync::Arc::clone(&self.audio),
            position: self.position,
            source_offset: self.source_offset,
            start_marker: self.start_marker,
            duration: self.duration,
            loop_enabled: self.loop_enabled,
            loop_start: self.loop_start,
            loop_end: self.loop_end,
            linear_gain: self.gain_db.linear(),
            fades: self.fades,
            playback_direction: self.playback_direction,
            warp_markers: self.warp_markers.clone(),
        }
    }
}

impl UiNoteClip {
    /// The clip with all its notes as one insertion.
    pub fn add_command(&self, track_id: TrackId) -> EngineCommand {
        self.add_command_beside(track_id, 0)
    }

    /// Adds the clip to a lane already holding `resident` note clips, with
    /// storage for all of them, so the engine never has to wait for the UI
    /// to hand it more.
    pub fn add_command_beside(&self, track_id: TrackId, resident: usize) -> EngineCommand {
        EngineCommand::AddNoteClip {
            track_id,
            insertion: PreparedNoteClipInsertion::with_capacity(
                self.id,
                self.position_beats,
                self.duration_beats,
                self.start_marker_beats,
                self.loop_enabled,
                self.loop_start_beats,
                self.loop_end_beats,
                self.groove_grid,
                resident + 1,
            )
            .with_notes(self.notes.clone()),
        }
    }
}
