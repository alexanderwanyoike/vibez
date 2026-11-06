//! Engine edits for a Clip whose edges moved while its content stayed put.

use std::sync::Arc;

use vibez_core::id::TrackId;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{UiClip, UiNoteClip};

/// Brings the engine's `old` to `new` in place, so the Clip keeps sounding
/// through a trim. Returns `false`, sending nothing, when `new` differs in
/// more than its edges and has to be added afresh.
pub(super) fn sync_audio_edges(
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    old: &UiClip,
    new: &UiClip,
) -> bool {
    let same_media = old.id == new.id
        && Arc::ptr_eq(&old.audio, &new.audio)
        && old.loop_enabled == new.loop_enabled
        && old.gain_db == new.gain_db
        && old.playback_direction == new.playback_direction;
    if !same_media {
        return false;
    }
    if new.position != old.position {
        engine.send(EngineCommand::MoveClip {
            track_id,
            clip_id: new.id,
            new_position: new.position,
        });
    }
    engine.send(new.bounds_command(track_id));
    if new.warp_markers != old.warp_markers {
        engine.send(EngineCommand::SetClipWarpMarkers {
            track_id,
            clip_id: new.id,
            warp_markers: new.warp_markers.clone(),
        });
    }
    if new.fades != old.fades {
        engine.send(EngineCommand::SetClipFades {
            track_id,
            clip_id: new.id,
            fades: new.fades,
        });
    }
    true
}

/// Note-clip counterpart of [`sync_audio_edges`].
pub(super) fn sync_note_edges(
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    old: &UiNoteClip,
    new: &UiNoteClip,
) -> bool {
    let same_content = old.id == new.id
        && old.notes == new.notes
        && old.loop_enabled == new.loop_enabled
        && old.groove_grid == new.groove_grid;
    if !same_content {
        return false;
    }
    if new.position_beats != old.position_beats {
        engine.send(EngineCommand::MoveNoteClip {
            track_id,
            clip_id: new.id,
            new_position_beats: new.position_beats,
        });
    }
    engine.send(new.bounds_command(track_id));
    true
}
