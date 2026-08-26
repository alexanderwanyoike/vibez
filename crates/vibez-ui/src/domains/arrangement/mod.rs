//! Arrange editing domain.
//!
//! Project Tracks are supplied explicitly from their project-wide store;
//! Arrange owns only timeline content and editor selection. Track lifecycle
//! and mixing messages originate here today because Arrange exposes those
//! controls, but they mutate the separate `ProjectTracksState`.

use std::collections::HashSet;

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::TrackKind;
use vibez_core::track::{AudioInputRoute, InputMonitoring};
use vibez_engine::commands::EngineCommand;

use super::timeline_editor::TimelineEditorAdapter;
use super::EngineHandle;
use crate::state::{
    ArrangementSelection, ArrangementState, ProjectTrack, ProjectTracksState, TimelineEditorState,
    TrackTimelineContent, UiNoteClip,
};

mod messages;
pub use messages::{
    ArrangementAction, ArrangementCtx, ArrangementMsg, ClipRenderedGeometry,
    ClipTransposeRenderRequest,
};
mod clipboard;
mod crossfades;

/// Every channel carries a flat SSL-style EQ. Also used for the master
/// bus, which is why it is crate-visible.
pub(crate) fn attach_channel_eq(engine: &mut impl EngineHandle, track: &mut ProjectTrack) {
    let effect_id = vibez_core::id::EffectId::new();
    let effect_type = vibez_core::effect::EffectType::Eq;
    let descriptors = vibez_dsp::factory::create_effect(effect_type, 48_000.0).param_descriptors();
    let params: Vec<f32> = descriptors.iter().map(|d| d.default).collect();
    track.effects.push(crate::state::UiEffect {
        id: effect_id,
        effect_type,
        bypass: false,
        params,
        descriptors,
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
    });
    engine.send(EngineCommand::AddEffect {
        track_id: track.id,
        effect_id,
        effect_type,
        position: None,
    });
}

impl ProjectTracksState {
    /// First track number with no name clash for the given prefix.
    pub fn next_unique_track_number(&mut self, prefix: &str) -> u32 {
        loop {
            let candidate = self.next_track_number;
            let name = format!("{prefix} {candidate}");
            if !self.tracks.iter().any(|t| t.name == name) {
                return candidate;
            }
            self.next_track_number += 1;
        }
    }

    fn move_track(&mut self, track_id: TrackId, up: bool, engine: &mut impl EngineHandle) {
        if let Some(idx) = self.tracks.iter().position(|t| t.id == track_id) {
            let target = if up {
                idx.checked_sub(1)
            } else if idx + 1 < self.tracks.len() {
                Some(idx + 1)
            } else {
                None
            };
            if let Some(target) = target {
                self.tracks.swap(idx, target);
                let order: Vec<TrackId> = self.tracks.iter().map(|t| t.id).collect();
                engine.send(EngineCommand::ReorderTracks(order));
            }
        }
    }
}

impl TimelineEditorState {
    pub(super) fn clear_time_selection(&mut self) {
        self.time_selection_active = false;
        self.time_selection_track = None;
        self.marquee = None;
    }

    fn find_content(&self, track_id: TrackId) -> Option<&TrackTimelineContent> {
        self.timeline.get(track_id)
    }

    fn find_content_mut(&mut self, track_id: TrackId) -> Option<&mut TrackTimelineContent> {
        Arc::make_mut(&mut self.timeline).get_mut(track_id)
    }
}

mod project_tracks;

impl TimelineEditorState {
    pub fn update(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        debug_assert!(msg.is_timeline_editor_message());
        let _ = ctx.samples_per_beat; // used by clip arms below
        let mut action = ArrangementAction::default();
        match msg {
            ArrangementMsg::RenameClip(track_id, clip_id, new_name) => {
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                        clip.name = new_name.clone();
                    }
                    if let Some(clip) = track.note_clips.iter_mut().find(|clip| clip.id == clip_id)
                    {
                        clip.name = new_name;
                    }
                }
            }
            ArrangementMsg::RemoveClip(track_id, clip_id) => {
                self.unlink_crossfades_for_clip(engine, track_id, clip_id);
                engine.send(EngineCommand::RemoveClip(track_id, clip_id));
                if let Some(track) = self.find_content_mut(track_id) {
                    track.clips.retain(|c| c.id != clip_id);
                }
                // Clear from multi-selection if this clip was selected
                self.selected_clips
                    .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
                if self.selected_transient_marker.is_some_and(
                    |(selected_track, selected_clip, _)| {
                        selected_track == track_id && selected_clip == clip_id
                    },
                ) {
                    self.selected_transient_marker = None;
                }
            }
            ArrangementMsg::ToggleClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled {
                            clip.reset_loop_region_to_clip();
                        }
                        cmd_data = Some((clip.loop_enabled, clip.loop_start, clip.loop_end));
                    }
                }
                if let Some((enabled, loop_start, loop_end)) = cmd_data {
                    engine.send(EngineCommand::SetClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start,
                        loop_end,
                    });
                }
            }
            ArrangementMsg::ToggleClipReverse(track_id, clip_id) => {
                if let Some(clip) = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                {
                    clip.playback_direction = clip.playback_direction.toggled();
                    engine.send(EngineCommand::SetClipPlaybackDirection {
                        track_id,
                        clip_id,
                        direction: clip.playback_direction,
                    });
                    action.status = Some(match clip.playback_direction {
                        vibez_core::track::ClipPlaybackDirection::Forward => {
                            "Audio Clip plays forward".into()
                        }
                        vibez_core::track::ClipPlaybackDirection::Reverse => {
                            "Audio Clip plays in reverse".into()
                        }
                    });
                }
            }
            message @ (ArrangementMsg::SelectTransientMarker { .. }
            | ArrangementMsg::AddTransientMarker { .. }
            | ArrangementMsg::MoveTransientMarker { .. }
            | ArrangementMsg::RemoveTransientMarker { .. }
            | ArrangementMsg::ReplaceDetectedTransientMarkers { .. }) => {
                return self.update_transient_markers(message);
            }
            message @ (ArrangementMsg::SelectWarpMarker { .. }
            | ArrangementMsg::AddWarpMarker { .. }
            | ArrangementMsg::MoveWarpMarker { .. }
            | ArrangementMsg::RemoveWarpMarker { .. }) => {
                return self.update_warp_markers(engine, message);
            }
            ArrangementMsg::SetClipLoopRegion {
                track_id,
                clip_id,
                loop_start,
                loop_end,
            } => {
                let mut command = None;
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        if clip.set_loop_region(loop_start, loop_end) {
                            command = Some(clip.loop_enabled);
                        }
                    }
                }
                if let Some(enabled) = command {
                    engine.send(EngineCommand::SetClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start,
                        loop_end,
                    });
                }
            }
            ArrangementMsg::SetClipStartMarker {
                track_id,
                clip_id,
                start_marker,
            } => {
                let mut changed = false;
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                        changed = clip.set_start_marker(start_marker);
                    }
                }
                if changed {
                    engine.send(EngineCommand::SetClipStartMarker {
                        track_id,
                        clip_id,
                        start_marker,
                    });
                }
            }
            ArrangementMsg::SelectArrangementClip {
                selection,
                shift_held,
            } => {
                self.discard_audio_clip_inspector_edits();
                self.selected_transient_marker = None;
                // Clicking a clip switches the editor back to clip selection.
                // Leaving an older time range active makes split/cut commands
                // silently operate on that range instead of the visible clip
                // selection.
                self.clear_time_selection();
                if shift_held {
                    // Toggle in/out of selection set
                    if !self.selected_clips.remove(&selection) {
                        self.selected_clips.insert(selection);
                    }
                } else {
                    // Replace selection
                    self.selected_clips.clear();
                    self.selected_clips.insert(selection);
                }
                action.focus_clip_tab = true;
                // Also update track selection and note clip selection for detail panel
                match selection {
                    ArrangementSelection::AudioClip { track_id, .. } => {
                        self.selected_track = Some(track_id);
                        // Clear note clip selection when an audio clip is selected
                        self.selected_note_clip = None;
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        self.selected_track = Some(track_id);
                        self.selected_note_clip = Some((track_id, clip_id));
                    }
                }
            }
            ArrangementMsg::MoveAudioClip {
                track_id,
                clip_id,
                new_position,
            } => {
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
            ArrangementMsg::MoveNoteClipPosition {
                track_id,
                clip_id,
                new_position_beats,
            } => {
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
            ArrangementMsg::ResizeAudioClip {
                track_id,
                clip_id,
                new_duration,
            } => {
                self.discard_audio_clip_inspector_edits_for(clip_id);
                return self.op_resize_audio_clip(engine, ctx, track_id, clip_id, new_duration);
            }
            ArrangementMsg::SetAudioClipFade {
                track_id,
                clip_id,
                edge,
                frames,
            } => {
                let changes_audible_fades = self
                    .find_content(track_id)
                    .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                    .is_some_and(|clip| {
                        let next = match edge {
                            crate::state::AudioClipFadeEdge::In => {
                                clip.fades.with_fade_in(frames, clip.duration)
                            }
                            crate::state::AudioClipFadeEdge::Out => {
                                clip.fades.with_fade_out(frames, clip.duration)
                            }
                        };
                        next.fade_in_frames() != clip.fades.fade_in_frames()
                            || next.fade_out_frames() != clip.fades.fade_out_frames()
                    });
                if !changes_audible_fades {
                    return ArrangementAction::default();
                }
                self.unlink_crossfade_edge_for_clip(engine, track_id, clip_id, edge);
                if let Some(clip) = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                {
                    let fades = match edge {
                        crate::state::AudioClipFadeEdge::In => {
                            clip.fades.with_fade_in(frames, clip.duration)
                        }
                        crate::state::AudioClipFadeEdge::Out => {
                            clip.fades.with_fade_out(frames, clip.duration)
                        }
                    };
                    clip.fades = fades;
                    engine.send(EngineCommand::SetClipFades {
                        track_id,
                        clip_id,
                        fades,
                    });
                    self.discard_audio_clip_inspector_edits_for(clip_id);
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::MoveClipToTrack {
                source_track,
                target_track,
                clip_id,
                is_note_clip,
            } => {
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
                            start_marker_beats: clip.start_marker_beats,
                            track_id: target_track,
                            clip_id,
                            position_beats: clip.position_beats,
                            duration_beats: clip.duration_beats,
                            loop_enabled: clip.loop_enabled,
                            loop_start_beats: clip.loop_start_beats,
                            loop_end_beats: clip.loop_end_beats,
                            groove_grid: clip.groove_grid,
                        });
                        for note in &clip.notes {
                            engine.send(EngineCommand::AddNote {
                                track_id: target_track,
                                clip_id,
                                note: *note,
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
            ArrangementMsg::DeleteSelectedClip => {
                let selections: Vec<_> = self.selected_clips.drain().collect();
                if !selections.is_empty() {
                    for selection in &selections {
                        match selection {
                            ArrangementSelection::AudioClip { track_id, clip_id } => {
                                self.unlink_crossfades_for_clip(engine, *track_id, *clip_id);
                                engine.send(EngineCommand::RemoveClip(*track_id, *clip_id));
                                if let Some(track) = self.find_content_mut(*track_id) {
                                    track.clips.retain(|c| c.id != *clip_id);
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                engine.send(EngineCommand::RemoveNoteClip(*track_id, *clip_id));
                                if let Some(track) = self.find_content_mut(*track_id) {
                                    track.note_clips.retain(|c| c.id != *clip_id);
                                }
                                if self
                                    .selected_note_clip
                                    .is_some_and(|(tid, cid)| tid == *track_id && cid == *clip_id)
                                {
                                    self.selected_note_clip = None;
                                }
                            }
                        }
                    }
                    let count = selections.len();
                    action.status = Some(if count == 1 {
                        "Deleted clip".to_string()
                    } else {
                        format!("Deleted {count} clips")
                    });
                }
            }
            ArrangementMsg::DuplicateSelectedClip => {
                let selections: Vec<_> = self.selected_clips.iter().copied().collect();
                if !selections.is_empty() {
                    let mut new_selections = HashSet::new();
                    for selection in &selections {
                        match selection {
                            ArrangementSelection::AudioClip { track_id, clip_id } => {
                                let duplicate = self.find_content(*track_id).and_then(|track| {
                                    track.clips.iter().find(|c| c.id == *clip_id).map(|clip| {
                                        let mut duplicate = clip.clone();
                                        duplicate.id = ClipId::new();
                                        duplicate.name = clip.name.clone();
                                        duplicate.position =
                                            clip.position.saturating_add(clip.duration);
                                        duplicate.fades = duplicate.fades.unlinked();
                                        duplicate
                                    })
                                });
                                if let Some(duplicate) = duplicate {
                                    engine.send(EngineCommand::AddClip {
                                        track_id: *track_id,
                                        clip_id: duplicate.id,
                                        audio: Arc::clone(&duplicate.audio),
                                        position: duplicate.position,
                                        source_offset: duplicate.source_offset,
                                        start_marker: duplicate.start_marker,
                                        duration: duplicate.duration,
                                        loop_enabled: duplicate.loop_enabled,
                                        loop_start: duplicate.loop_start,
                                        loop_end: duplicate.loop_end,
                                        linear_gain: duplicate.gain_db.linear(),
                                        fades: duplicate.fades,
                                        playback_direction: duplicate.playback_direction,
                                        warp_markers: duplicate.warp_markers.clone(),
                                    });
                                    let new_id = duplicate.id;
                                    if let Some(track) = self.find_content_mut(*track_id) {
                                        track.clips.push(duplicate);
                                    }
                                    new_selections.insert(ArrangementSelection::AudioClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                    });
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                let duplicate =
                                    self.find_content(*track_id).and_then(|track| {
                                        track.note_clips.iter().find(|c| c.id == *clip_id).map(
                                            |clip| {
                                                let mut duplicate = clip.clone();
                                                duplicate.id = ClipId::new();
                                                duplicate.name = clip.name.clone();
                                                duplicate.position_beats =
                                                    clip.position_beats + clip.duration_beats;
                                                duplicate.selected_notes.clear();
                                                duplicate
                                            },
                                        )
                                    });
                                if let Some(duplicate) = duplicate {
                                    engine.send(EngineCommand::AddNoteClip {
                                        start_marker_beats: duplicate.start_marker_beats,
                                        track_id: *track_id,
                                        clip_id: duplicate.id,
                                        position_beats: duplicate.position_beats,
                                        duration_beats: duplicate.duration_beats,
                                        loop_enabled: duplicate.loop_enabled,
                                        loop_start_beats: duplicate.loop_start_beats,
                                        loop_end_beats: duplicate.loop_end_beats,
                                        groove_grid: duplicate.groove_grid,
                                    });
                                    for note in &duplicate.notes {
                                        engine.send(EngineCommand::AddNote {
                                            track_id: *track_id,
                                            clip_id: duplicate.id,
                                            note: *note,
                                        });
                                    }
                                    let new_id = duplicate.id;
                                    if let Some(track) = self.find_content_mut(*track_id) {
                                        track.note_clips.push(duplicate);
                                    }
                                    new_selections.insert(ArrangementSelection::NoteClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                    });
                                }
                            }
                        }
                    }
                    // Select the new copies
                    self.selected_clips = new_selections;
                    let count = selections.len();
                    action.status = Some(if count == 1 {
                        "Duplicated clip".to_string()
                    } else {
                        format!("Duplicated {count} clips")
                    });
                }
            }
            ArrangementMsg::CopySelectedClips => {
                unreachable!("clipboard messages are resolved at the application boundary")
            }
            ArrangementMsg::CutSelectedClips | ArrangementMsg::PasteClips => {
                unreachable!("clipboard messages are resolved at the application boundary")
            }
            ArrangementMsg::ToggleSelectedClipLoop => {
                return self.op_toggle_selected_clip_loop(engine);
            }
            ArrangementMsg::ResizeSelectedClips {
                anchor,
                new_duration_beats,
            } => {
                return self.op_resize_selected_clips(
                    project_tracks,
                    engine,
                    ctx,
                    anchor,
                    new_duration_beats,
                );
            }
            ArrangementMsg::SetTimeSelection {
                start_beats,
                end_beats,
                track_id,
            } => {
                self.selection_start_beats = start_beats;
                self.selection_end_beats = end_beats;
                self.time_selection_active = true;
                self.time_selection_track = track_id;
                if let Some(tid) = track_id {
                    self.selected_track = Some(tid);
                }
            }
            ArrangementMsg::SetSelectionAsLoop => {
                // Transport owns the loop; hand the region over.
                if self.time_selection_active
                    && self.selection_end_beats > self.selection_start_beats
                {
                    action.loop_from_selection =
                        Some((self.selection_start_beats, self.selection_end_beats));
                }
            }
            ArrangementMsg::MarqueeSelect {
                anchor_track,
                start_beats,
                end_beats,
                top_y,
                bottom_y,
                track_ids,
                additive,
            } => {
                return self.op_marquee_select(
                    ctx,
                    anchor_track,
                    start_beats,
                    end_beats,
                    top_y,
                    bottom_y,
                    &track_ids,
                    additive,
                );
            }
            ArrangementMsg::EndMarqueeSelect => {
                self.marquee = None;
            }
            ArrangementMsg::SelectAllClips => {
                self.clear_time_selection();
                self.selected_clips =
                    self.timeline
                        .by_track
                        .iter()
                        .flat_map(|(track_id, content)| {
                            let audio =
                                content
                                    .clips
                                    .iter()
                                    .map(|clip| ArrangementSelection::AudioClip {
                                        track_id: *track_id,
                                        clip_id: clip.id,
                                    });
                            let notes = content.note_clips.iter().map(|clip| {
                                ArrangementSelection::NoteClip {
                                    track_id: *track_id,
                                    clip_id: clip.id,
                                }
                            });
                            audio.chain(notes)
                        })
                        .collect();
                action.focus_clip_tab = !self.selected_clips.is_empty();
            }
            ArrangementMsg::SetTimeSelectionActive(active) => {
                if active {
                    self.time_selection_active = true;
                } else {
                    self.clear_time_selection();
                }
            }
            ArrangementMsg::DuplicateNoteClip(track_id, clip_id) => {
                let new_clip_id = ClipId::new();
                let mut new_clip_data = None;

                if let Some(track) = self.find_content(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let new_pos = clip.position_beats + clip.duration_beats;
                        new_clip_data = Some((
                            UiNoteClip {
                                id: new_clip_id,
                                name: clip.name.clone(),
                                position_beats: new_pos,
                                duration_beats: clip.duration_beats,
                                notes: clip.notes.clone(),
                                selected_notes: HashSet::new(),
                                start_marker_beats: clip.start_marker_beats,
                                loop_enabled: clip.loop_enabled,
                                loop_start_beats: clip.loop_start_beats,
                                loop_end_beats: clip.loop_end_beats,
                                groove_grid: clip.groove_grid,
                            },
                            new_pos,
                            clip.duration_beats,
                            clip.notes.clone(),
                            clip.start_marker_beats,
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                            clip.groove_grid,
                        ));
                    }
                }

                if let Some((
                    new_clip,
                    pos,
                    dur,
                    notes,
                    start_marker_beats,
                    loop_enabled,
                    loop_start,
                    loop_end,
                    groove_grid,
                )) = new_clip_data
                {
                    if let Some(track) = self.find_content_mut(track_id) {
                        track.note_clips.push(new_clip);
                    }
                    engine.send(EngineCommand::AddNoteClip {
                        start_marker_beats,
                        track_id,
                        clip_id: new_clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled,
                        loop_start_beats: loop_start,
                        loop_end_beats: loop_end,
                        groove_grid,
                    });
                    for note in &notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id: new_clip_id,
                            note: *note,
                        });
                    }
                    self.selected_note_clip = Some((track_id, new_clip_id));
                    action.status = Some("Duplicated clip".to_string());
                }
            }
            ArrangementMsg::SplitAudioClip {
                track_id,
                clip_id,
                split_position,
            } => {
                return self.op_split_audio_clip(engine, ctx, track_id, clip_id, split_position);
            }
            ArrangementMsg::SplitNoteClip {
                track_id,
                clip_id,
                split_beat,
            } => {
                return self.op_split_note_clip(engine, ctx, track_id, clip_id, split_beat);
            }
            ArrangementMsg::SplitSelectedAtPlayhead => {
                if self.time_selection_active
                    && self.selection_end_beats > self.selection_start_beats
                {
                    return self.update(
                        project_tracks,
                        ArrangementMsg::SplitClipsAtRegion {
                            start_beats: self.selection_start_beats,
                            end_beats: self.selection_end_beats,
                            track_id: self.time_selection_track,
                        },
                        engine,
                        ctx,
                    );
                }

                let clips: Vec<_> = self.selected_clips.iter().copied().collect();
                for selection in clips {
                    match selection {
                        ArrangementSelection::AudioClip { track_id, clip_id } => {
                            let _ = self.update(
                                project_tracks,
                                ArrangementMsg::SplitAudioClip {
                                    track_id,
                                    clip_id,
                                    split_position: ctx.playhead_samples,
                                },
                                engine,
                                ctx,
                            );
                        }
                        ArrangementSelection::NoteClip { track_id, clip_id } => {
                            let _ = self.update(
                                project_tracks,
                                ArrangementMsg::SplitNoteClip {
                                    track_id,
                                    clip_id,
                                    split_beat: ctx.playhead_beats,
                                },
                                engine,
                                ctx,
                            );
                        }
                    }
                }
            }
            ArrangementMsg::JoinSelectedClips => {
                return self.op_join_selected_clips(engine, ctx);
            }
            ArrangementMsg::CrossfadeSelectedAudioClips => {
                return self.crossfade_selected_audio_clips(engine);
            }
            ArrangementMsg::TrimSelectedByTrackMutes => {
                return self.op_trim_selected_by_track_mutes(engine, ctx);
            }
            ArrangementMsg::DeleteClipsInRegion {
                start_beats,
                end_beats,
                track_id,
            } => {
                return self.op_delete_clips_in_region(
                    engine,
                    ctx,
                    start_beats,
                    end_beats,
                    track_id,
                );
            }
            ArrangementMsg::SplitClipsAtRegion {
                start_beats,
                end_beats,
                track_id,
            } => {
                return self.op_split_clips_at_region(
                    engine,
                    ctx,
                    start_beats,
                    end_beats,
                    track_id,
                );
            }
            ArrangementMsg::CreateClipFromSelection => {
                return self.op_create_clip_from_selection(project_tracks, engine, ctx);
            }
            ArrangementMsg::CreateNoteClipFromSelection(track_id) => {
                return self.op_create_note_clip_from_selection(
                    project_tracks,
                    engine,
                    ctx,
                    track_id,
                );
            }
            ArrangementMsg::AudioClipInspectorInputChanged {
                clip_id,
                field,
                text,
            } => {
                self.audio_clip_inspector_edits
                    .insert((clip_id, field), text);
            }
            ArrangementMsg::DiscardAudioClipInspectorEdit { clip_id, field } => {
                self.audio_clip_inspector_edits.remove(&(clip_id, field));
                if field == crate::state::AudioClipInspectorField::Transpose {
                    self.audio_clip_transpose_debounce.remove(&clip_id);
                }
            }
            ArrangementMsg::SubmitAudioClipInspectorField {
                track_id,
                clip_id,
                field,
            } => return self.commit_audio_clip_inspector_field(engine, track_id, clip_id, field),
            ArrangementMsg::SetAudioClipRotaryValue {
                track_id,
                clip_id,
                field,
                value,
            } => return self.set_audio_clip_rotary_value(engine, track_id, clip_id, field, value),
            ArrangementMsg::PreviewAudioClipRotaryValue {
                track_id,
                clip_id,
                field,
                value,
            } => {
                return self.preview_audio_clip_rotary_value(track_id, clip_id, field, value);
            }
            ArrangementMsg::SetClipNominalBpm {
                track_id,
                clip_id,
                bpm,
            } => {
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(bpm);
                    }
                }
                action.status = Some(format!("Clip BPM set to {:.1}", bpm));
                action.mark_dirty = true;
            }
            ArrangementMsg::ClearClipWarp { track_id, clip_id } => {
                return self.apply_clear_clip_warp(engine, track_id, clip_id);
            }
            _ => unreachable!("Project Track messages never enter the Timeline Editor"),
        }
        action
    }
}

mod audio_clip_inspector;
mod fragment_geometry;
mod media_ops;
mod ops;
mod transient_markers;
mod warp_markers;
mod warp_ops;

#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
