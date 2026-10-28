//! Moving Arrangement clips in time and between Project Track lanes.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use crate::state::{ArrangementSelection, ProjectTracksState, TimelineEditorState};

use super::{ArrangementCtx, EngineHandle};

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

    pub(super) fn move_selected_clips(
        &mut self,
        project_tracks: &ProjectTracksState,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        anchor: ClipId,
        new_position_beats: f64,
        target_track: TrackId,
    ) {
        if ctx.samples_per_beat <= 0.0 {
            return;
        }
        let Some(anchor) = self.locate_clip(anchor) else {
            return;
        };
        let targets: Vec<ArrangementSelection> = if self.selected_clips.contains(&anchor) {
            self.selected_clips.iter().copied().collect()
        } else {
            vec![anchor]
        };
        let beats_of = |this: &Self, selection: ArrangementSelection| match selection {
            ArrangementSelection::AudioClip { track_id, clip_id } => this
                .find_content(track_id)
                .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                .map(|clip| clip.position as f64 / ctx.samples_per_beat),
            ArrangementSelection::NoteClip { track_id, clip_id } => this
                .find_content(track_id)
                .and_then(|content| content.note_clips.iter().find(|clip| clip.id == clip_id))
                .map(|clip| clip.position_beats),
        };
        let Some(anchor_beats) = beats_of(self, anchor) else {
            return;
        };
        let earliest = targets
            .iter()
            .filter_map(|selection| beats_of(self, *selection))
            .fold(f64::INFINITY, f64::min);
        let delta_beats = (new_position_beats - anchor_beats).max(-earliest);
        let delta_samples = (delta_beats * ctx.samples_per_beat).round() as i64;

        let lane_of = |track_id: TrackId| {
            project_tracks
                .tracks
                .iter()
                .position(|track| track.id == track_id)
        };
        let lane_shift = lane_of(target_track)
            .zip(lane_of(anchor.track_id()))
            .map_or(0, |(target, current)| target as isize - current as isize);
        // A lane change moves every clip or none, so the selection never
        // splits apart on a lane that cannot hold one of its clips.
        let destinations: Option<Vec<TrackId>> = (lane_shift != 0)
            .then(|| {
                targets
                    .iter()
                    .map(|selection| {
                        let lane = lane_of(selection.track_id())?.checked_add_signed(lane_shift)?;
                        let track = project_tracks.tracks.get(lane)?;
                        (track.kind.is_midi() == selection.is_note_clip()).then_some(track.id)
                    })
                    .collect()
            })
            .flatten();

        for (index, selection) in targets.iter().enumerate() {
            match *selection {
                ArrangementSelection::AudioClip { track_id, clip_id } if delta_samples != 0 => {
                    if let Some(position) = self
                        .find_content(track_id)
                        .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                        .map(|clip| clip.position)
                    {
                        let moved = (position as i64 + delta_samples).max(0) as u64;
                        self.move_audio_clip(engine, track_id, clip_id, moved);
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } if delta_beats != 0.0 => {
                    if let Some(beats) = beats_of(self, *selection) {
                        self.move_note_clip(
                            engine,
                            track_id,
                            clip_id,
                            (beats + delta_beats).max(0.0),
                        );
                    }
                }
                _ => {}
            }
            if let Some(destination) = destinations.as_ref().map(|lanes| lanes[index]) {
                self.move_clip_to_track(
                    engine,
                    selection.track_id(),
                    destination,
                    selection.clip_id(),
                    selection.is_note_clip(),
                );
            }
        }
        if destinations.is_some() {
            self.selected_track = Some(target_track);
        }
    }

    fn locate_clip(&self, clip_id: ClipId) -> Option<ArrangementSelection> {
        self.timeline
            .by_track
            .iter()
            .find_map(|(&track_id, content)| {
                if content.clips.iter().any(|clip| clip.id == clip_id) {
                    Some(ArrangementSelection::AudioClip { track_id, clip_id })
                } else if content.note_clips.iter().any(|clip| clip.id == clip_id) {
                    Some(ArrangementSelection::NoteClip { track_id, clip_id })
                } else {
                    None
                }
            })
    }
}
