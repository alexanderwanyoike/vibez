//! Engine edits for a Clip whose edges moved while its content stayed put.

use std::sync::Arc;

use vibez_core::id::TrackId;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::UiClip;

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
