//! Brings the engine's copy of one channel's timeline content to a target by swapping only what differs.

use vibez_core::id::TrackId;
use vibez_engine::clip_insertion::PreparedAutomationLaneInsertion;
use vibez_engine::commands::EngineCommand;

use crate::state::TrackTimelineContent;

use super::EngineHandle;

/// Clips and lanes equal in both stay untouched in the engine, so they keep
/// sounding; a changed clip is removed and re-added whole.
pub fn swap_changed_content(
    engine: &mut impl EngineHandle,
    track_id: TrackId,
    current: &TrackTimelineContent,
    target: &TrackTimelineContent,
) {
    if current == target {
        return;
    }
    for clip in &current.clips {
        if !target.clips.contains(clip) {
            engine.send(EngineCommand::RemoveClip(track_id, clip.id));
        }
    }
    for clip in &current.note_clips {
        if !target.note_clips.contains(clip) {
            engine.send(EngineCommand::RemoveNoteClip(track_id, clip.id));
        }
    }
    for lane in &current.automation {
        if !target.automation.iter().any(|kept| kept.id == lane.id) {
            engine.send(EngineCommand::RemoveAutomationLane {
                track_id,
                lane_id: lane.id,
            });
        }
    }
    for clip in &target.clips {
        if !current.clips.contains(clip) {
            engine.add_audio_clip(track_id, clip);
        }
    }
    for clip in &target.note_clips {
        if !current.note_clips.contains(clip) {
            engine.send(clip.add_command(track_id));
        }
    }
    for lane in &target.automation {
        if !current.automation.contains(lane) {
            engine.send(EngineCommand::SetAutomationLane {
                track_id,
                insertion: PreparedAutomationLaneInsertion::new(lane.clone()),
            });
        }
    }
}
