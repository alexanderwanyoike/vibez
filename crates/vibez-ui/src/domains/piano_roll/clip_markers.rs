use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use super::{find_note_clip_mut, EngineHandle, TimelineContent};

pub(super) fn toggle_loop(
    tracks: &mut TimelineContent,
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    clip_id: ClipId,
) {
    let Some(clip) = find_note_clip_mut(tracks, track_id, clip_id) else {
        return;
    };
    clip.loop_enabled = !clip.loop_enabled;
    if clip.loop_enabled {
        clip.reset_loop_region_to_clip();
    }
    engine.send(EngineCommand::SetNoteClipLoop {
        track_id,
        clip_id,
        enabled: clip.loop_enabled,
        loop_start_beats: clip.loop_start_beats,
        loop_end_beats: clip.loop_end_beats,
    });
}

pub(super) fn set_loop_region(
    tracks: &mut TimelineContent,
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    clip_id: ClipId,
    loop_start_beats: f64,
    loop_end_beats: f64,
) {
    let Some(clip) = find_note_clip_mut(tracks, track_id, clip_id) else {
        return;
    };
    if !clip.set_loop_region(loop_start_beats, loop_end_beats) {
        return;
    }
    engine.send(EngineCommand::SetNoteClipLoop {
        track_id,
        clip_id,
        enabled: clip.loop_enabled,
        loop_start_beats,
        loop_end_beats,
    });
}

pub(super) fn set_start(
    tracks: &mut TimelineContent,
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    clip_id: ClipId,
    start_marker_beats: f64,
) {
    let Some(clip) = find_note_clip_mut(tracks, track_id, clip_id) else {
        return;
    };
    if !clip.set_start_marker(start_marker_beats) {
        return;
    }
    engine.send(EngineCommand::SetNoteClipStartMarker {
        track_id,
        clip_id,
        start_marker_beats,
    });
}
