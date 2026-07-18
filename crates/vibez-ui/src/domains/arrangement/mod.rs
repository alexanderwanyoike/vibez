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
use vibez_engine::commands::EngineCommand;

use super::timeline_editor::TimelineEditorAdapter;
use super::EngineHandle;
use crate::state::{
    ArrangementSelection, ArrangementState, ProjectTrack, ProjectTracksState, TimelineEditorState,
    TrackTimelineContent, UiNoteClip,
};

mod messages;
pub use messages::{ArrangementAction, ArrangementCtx, ArrangementMsg};

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
    fn find_content(&self, track_id: TrackId) -> Option<&TrackTimelineContent> {
        self.timeline.get(track_id)
    }

    fn find_content_mut(&mut self, track_id: TrackId) -> Option<&mut TrackTimelineContent> {
        Arc::make_mut(&mut self.timeline).get_mut(track_id)
    }
}

impl ArrangementState {
    /// Arrange owns Project Track controls and resolves its local timeline
    /// before forwarding editor messages to the shared boundary.
    pub fn update(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        if msg.is_timeline_editor_message() {
            return self
                .resolve_timeline_mut()
                .editor
                .update(project_tracks, msg, engine, ctx);
        }

        let mut action = ArrangementAction::default();
        match msg {
            ArrangementMsg::AddTrack => {
                let track_num = project_tracks.next_unique_track_number("Track");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                project_tracks.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");
                engine.send(EngineCommand::AddTrack(id, name.clone()));
                let mut track = ProjectTrack::new(id, name, color_index);
                attach_channel_eq(engine, &mut track);
                project_tracks.tracks.push(track);
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", project_tracks.tracks.len()));
            }
            ArrangementMsg::AddMidiTrack | ArrangementMsg::AddInstrumentTrack => {
                let track_num = project_tracks.next_unique_track_number("MIDI");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                project_tracks.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("MIDI {track_num}");
                engine.send(EngineCommand::AddMidiTrack(id, name.clone()));
                let mut track =
                    ProjectTrack::new_instrument(id, name, TrackKind::Midi, color_index);
                track.has_instrument = false;
                attach_channel_eq(engine, &mut track);
                project_tracks.tracks.push(track);
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", project_tracks.tracks.len()));
            }
            ArrangementMsg::RequestRemoveTrack(track_id) => {
                if !track_id.is_master()
                    && project_tracks
                        .tracks
                        .iter()
                        .any(|track| track.id == track_id)
                {
                    self.pending_project_track_deletion = Some(track_id);
                }
            }
            ArrangementMsg::CancelRemoveTrack => {
                self.pending_project_track_deletion = None;
            }
            ArrangementMsg::ConfirmRemoveTrack(track_id) => {
                if track_id.is_master() {
                    return action;
                }
                if self.pending_project_track_deletion != Some(track_id) {
                    return action;
                }
                self.pending_project_track_deletion = None;
                let removed_name = project_tracks
                    .tracks
                    .iter()
                    .find(|track| track.id == track_id)
                    .map(|track| track.name.clone())
                    .unwrap_or_else(|| format!("{track_id}"));
                engine.send(EngineCommand::RemoveTrack(track_id));
                project_tracks.tracks.retain(|track| track.id != track_id);
                Arc::make_mut(&mut self.timeline).remove(track_id);
                if self.selected_track == Some(track_id) {
                    self.selected_track = project_tracks.tracks.first().map(|track| track.id);
                }
                if self
                    .selected_note_clip
                    .is_some_and(|(id, _)| id == track_id)
                {
                    self.selected_note_clip = None;
                }
                self.selected_clips.retain(|selection| match selection {
                    ArrangementSelection::AudioClip { track_id: id, .. }
                    | ArrangementSelection::NoteClip { track_id: id, .. } => *id != track_id,
                });
                action.close_track_guis = Some(track_id);
                action.remove_track_from_sections = Some(track_id);
                action.status = Some(format!(
                    "Removed {removed_name}. {} track(s) remain.",
                    project_tracks.tracks.len()
                ));
            }
            ArrangementMsg::SelectTrack(track_id) => self.selected_track = Some(track_id),
            ArrangementMsg::RenameTrack(track_id, new_name) => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.name = new_name;
                }
            }
            ArrangementMsg::MoveTrackUp(track_id) => {
                project_tracks.move_track(track_id, true, engine)
            }
            ArrangementMsg::MoveTrackDown(track_id) => {
                project_tracks.move_track(track_id, false, engine)
            }
            ArrangementMsg::MoveSelectedTrackUp => {
                if let Some(track_id) = self.selected_track {
                    project_tracks.move_track(track_id, true, engine);
                }
            }
            ArrangementMsg::MoveSelectedTrackDown => {
                if let Some(track_id) = self.selected_track {
                    project_tracks.move_track(track_id, false, engine);
                }
            }
            ArrangementMsg::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                engine.send(EngineCommand::SetTrackGain(track_id, gain));
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.gain = gain;
                }
            }
            ArrangementMsg::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                engine.send(EngineCommand::SetTrackPan(track_id, pan));
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.pan = pan;
                }
            }
            ArrangementMsg::SetTrackMute(track_id) => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.mute = !track.mute;
                    engine.send(EngineCommand::SetTrackMute(track_id, track.mute));
                }
            }
            ArrangementMsg::SetTrackSolo(track_id) => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.solo = !track.solo;
                    engine.send(EngineCommand::SetTrackSolo(track_id, track.solo));
                }
            }
            ArrangementMsg::AddBus => {
                let letter = (b'A' + (project_tracks.buses.len() % 26) as u8) as char;
                let id = TrackId::new();
                let name = format!("{letter} Return");
                engine.send(EngineCommand::AddBus(id, name.clone()));
                let color_index = ((project_tracks.buses.len() + 4) % 8) as u8;
                let mut bus = ProjectTrack::new(id, name.clone(), color_index);
                attach_channel_eq(engine, &mut bus);
                project_tracks.buses.push(bus);
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("Added {name}"));
            }
            ArrangementMsg::RemoveBus(bus_id) => {
                engine.send(EngineCommand::RemoveBus(bus_id));
                project_tracks.buses.retain(|bus| bus.id != bus_id);
                Arc::make_mut(&mut self.timeline).remove(bus_id);
                for track in &mut project_tracks.tracks {
                    track.sends.retain(|(id, _)| *id != bus_id);
                }
                for content in Arc::make_mut(&mut self.timeline).by_track.values_mut() {
                    content.automation.retain(|lane| {
                        lane.target != vibez_core::automation::AutomationTarget::Send { bus_id }
                    });
                }
                if self.selected_track == Some(bus_id) {
                    self.selected_track = project_tracks.tracks.first().map(|track| track.id);
                }
                action.close_track_guis = Some(bus_id);
                action.remove_track_from_sections = Some(bus_id);
                action.status = Some("Removed bus".to_string());
            }
            ArrangementMsg::SetSend {
                track_id,
                bus_id,
                amount,
            } => {
                let amount = amount.clamp(0.0, 1.0);
                if let Some(track) = project_tracks.tracks.iter_mut().find(|t| t.id == track_id) {
                    match track.sends.iter_mut().find(|(id, _)| *id == bus_id) {
                        Some(send) => send.1 = amount,
                        None => track.sends.push((bus_id, amount)),
                    }
                    engine.send(EngineCommand::SetSend {
                        track_id,
                        bus_id,
                        amount,
                    });
                }
            }
            ArrangementMsg::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }
            _ => unreachable!("editor messages are delegated before Arrange track handling"),
        }
        action
    }
}

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
                engine.send(EngineCommand::RemoveClip(track_id, clip_id));
                if let Some(track) = self.find_content_mut(track_id) {
                    track.clips.retain(|c| c.id != clip_id);
                }
                // Clear from multi-selection if this clip was selected
                self.selected_clips
                    .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
            }
            ArrangementMsg::ToggleClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled && clip.loop_end == 0 {
                            clip.loop_start = clip.source_offset;
                            clip.loop_end = clip.source_offset + clip.duration;
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
            ArrangementMsg::SetClipLoopRegion {
                track_id,
                clip_id,
                loop_start,
                loop_end,
            } => {
                let mut enabled = false;
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_start = loop_start;
                        clip.loop_end = loop_end;
                        enabled = clip.loop_enabled;
                    }
                }
                engine.send(EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start,
                    loop_end,
                });
            }
            ArrangementMsg::SelectArrangementClip {
                selection,
                shift_held,
            } => {
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
                return self.op_resize_audio_clip(engine, ctx, track_id, clip_id, new_duration);
            }
            ArrangementMsg::MoveClipToTrack {
                source_track,
                target_track,
                clip_id,
                is_note_clip,
            } => {
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
                            clip_id,
                            position_beats: clip.position_beats,
                            duration_beats: clip.duration_beats,
                            loop_enabled: clip.loop_enabled,
                            loop_start_beats: clip.loop_start_beats,
                            loop_end_beats: clip.loop_end_beats,
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
                            duration: clip.duration,
                            loop_enabled: clip.loop_enabled,
                            loop_start: clip.loop_start,
                            loop_end: clip.loop_end,
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
                                        duration: duplicate.duration,
                                        loop_enabled: duplicate.loop_enabled,
                                        loop_start: duplicate.loop_start,
                                        loop_end: duplicate.loop_end,
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
                                        track_id: *track_id,
                                        clip_id: duplicate.id,
                                        position_beats: duplicate.position_beats,
                                        duration_beats: duplicate.duration_beats,
                                        loop_enabled: duplicate.loop_enabled,
                                        loop_start_beats: duplicate.loop_start_beats,
                                        loop_end_beats: duplicate.loop_end_beats,
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
                return self.op_copy_selected_clips(ctx);
            }
            ArrangementMsg::CutSelectedClips => {
                let start = self.selection_start_beats;
                let end = self.selection_end_beats;
                let track_id = self.time_selection_track;
                let ranged = self.time_selection_active && end > start;
                let copy = self.op_copy_selected_clips(ctx);
                if copy.status.as_deref() == Some("Nothing to copy") {
                    return copy;
                }
                let mut result = if ranged {
                    self.op_delete_clips_in_region(engine, ctx, start, end, track_id)
                } else {
                    self.update(
                        project_tracks,
                        ArrangementMsg::DeleteSelectedClip,
                        engine,
                        ctx,
                    )
                };
                result.status = Some("Cut to clipboard".to_string());
                return result;
            }
            ArrangementMsg::PasteClipsAtPlayhead => {
                return self.op_paste_clips_at_playhead(engine, ctx);
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
            ArrangementMsg::SetTimeSelectionActive(active) => {
                self.time_selection_active = active;
                if !active {
                    self.time_selection_track = None;
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
                                loop_enabled: clip.loop_enabled,
                                loop_start_beats: clip.loop_start_beats,
                                loop_end_beats: clip.loop_end_beats,
                            },
                            new_pos,
                            clip.duration_beats,
                            clip.notes.clone(),
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }

                if let Some((new_clip, pos, dur, notes, loop_enabled, loop_start, loop_end)) =
                    new_clip_data
                {
                    if let Some(track) = self.find_content_mut(track_id) {
                        track.note_clips.push(new_clip);
                    }
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: new_clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled,
                        loop_start_beats: loop_start,
                        loop_end_beats: loop_end,
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
            ArrangementMsg::ClipBpmInputChanged {
                track_id: _,
                clip_id,
                text,
            } => {
                self.clip_bpm_edit.insert(clip_id, text);
            }
            ArrangementMsg::SubmitClipBpm { track_id, clip_id } => {
                let parsed = self
                    .clip_bpm_edit
                    .remove(&clip_id)
                    .and_then(|t| t.parse::<f64>().ok())
                    .filter(|b| *b > 0.0 && *b < 1_000.0);
                if let Some(bpm) = parsed {
                    if let Some(track) = self.find_content_mut(track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.original_bpm = Some(bpm);
                        }
                    }
                    action.status = Some(format!("Clip BPM set to {:.1}", bpm));
                    action.mark_dirty = true;
                }
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

mod media_ops;
mod ops;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
