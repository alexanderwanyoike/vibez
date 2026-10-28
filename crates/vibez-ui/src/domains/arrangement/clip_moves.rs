//! Moving Arrangement clips in time and between Project Track lanes.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use crate::state::{ArrangementSelection, TimelineEditorState};

use super::EngineHandle;

impl TimelineEditorState {
    pub(super) fn move_audio_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        new_position: u64,
    ) {
        self.unlink_crossfades_for_clip(engine, track_id, clip_id);
        if let Some(track) = self.find_content_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                clip.position = new_position;
            }
        }
        engine.send(EngineCommand::MoveClip {
            track_id,
            clip_id,
            new_position,
        });
        self.drag_resize_active = true;
    }

    pub(super) fn move_note_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        new_position_beats: f64,
    ) {
        if let Some(track) = self.find_content_mut(track_id) {
            if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                clip.position_beats = new_position_beats;
            }
        }
        engine.send(EngineCommand::MoveNoteClip {
            track_id,
            clip_id,
            new_position_beats,
        });
        self.drag_resize_active = true;
    }

    pub(super) fn move_clip_to_track(
        &mut self,
        engine: &mut impl EngineHandle,
        source_track: TrackId,
        target_track: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    ) {
        if !is_note_clip {
            self.unlink_crossfades_for_clip(engine, source_track, clip_id);
        }
        if is_note_clip {
            // Move note clip between instrument tracks
            let mut clip_data = None;
            if let Some(track) = self.find_content_mut(source_track) {
                if let Some(idx) = track.note_clips.iter().position(|c| c.id == clip_id) {
                    clip_data = Some(track.note_clips.remove(idx));
                }
            }
            if let Some(clip) = clip_data {
                // Remove from engine source track
                engine.send(EngineCommand::RemoveNoteClip(source_track, clip_id));
                // Add to engine target track
                engine.send(EngineCommand::AddNoteClip {
                    track_id: target_track,
                    insertion: vibez_engine::clip_insertion::PreparedNoteClipInsertion::new(
                        clip_id,
                        clip.position_beats,
                        clip.duration_beats,
                        clip.start_marker_beats,
                        clip.loop_enabled,
                        clip.loop_start_beats,
                        clip.loop_end_beats,
                        clip.groove_grid,
                    ),
                });
                for note in &clip.notes {
                    engine.send(EngineCommand::AddNote {
                        track_id: target_track,
                        clip_id,
                        insertion: vibez_engine::clip_insertion::PreparedNoteInsertion::new(*note),
                    });
                }
                // Add to UI target track
                if let Some(track) = self.find_content_mut(target_track) {
                    track.note_clips.push(clip);
                }
                // Update selection
                self.selected_clips.remove(&ArrangementSelection::NoteClip {
                    track_id: source_track,
                    clip_id,
                });
                self.selected_clips.insert(ArrangementSelection::NoteClip {
                    track_id: target_track,
                    clip_id,
                });
                self.selected_track = Some(target_track);
                self.selected_note_clip = Some((target_track, clip_id));
            }
        } else {
            // Move audio clip between audio tracks
            let mut clip_data = None;
            if let Some(track) = self.find_content_mut(source_track) {
                if let Some(idx) = track.clips.iter().position(|c| c.id == clip_id) {
                    clip_data = Some(track.clips.remove(idx));
                }
            }
            if let Some(clip) = clip_data {
                // Remove from engine source track
                engine.send(EngineCommand::RemoveClip(source_track, clip_id));
                // Add to engine target track
                engine.send(EngineCommand::AddClip {
                    insertion: vibez_engine::clip_insertion::PreparedClipInsertion::new(),
                    track_id: target_track,
                    clip_id,
                    audio: Arc::clone(&clip.audio),
                    position: clip.position,
                    source_offset: clip.source_offset,
                    start_marker: clip.start_marker,
                    duration: clip.duration,
                    loop_enabled: clip.loop_enabled,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                    linear_gain: clip.gain_db.linear(),
                    fades: clip.fades,
                    playback_direction: clip.playback_direction,
                    warp_markers: clip.warp_markers.clone(),
                });
                // Add to UI target track
                if let Some(track) = self.find_content_mut(target_track) {
                    track.clips.push(clip);
                }
                // Update selection
                self.selected_clips
                    .remove(&ArrangementSelection::AudioClip {
                        track_id: source_track,
                        clip_id,
                    });
                self.selected_clips.insert(ArrangementSelection::AudioClip {
                    track_id: target_track,
                    clip_id,
                });
                self.selected_track = Some(target_track);
            }
        }
    }
}
