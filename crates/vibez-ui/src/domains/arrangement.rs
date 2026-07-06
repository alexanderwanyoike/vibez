//! Arrangement domain, tranche one: track lifecycle and mixing
//! basics. Owns the track list (the shared model other domains
//! receive explicitly), selection, and track numbering.
//!
//! Clip operations (moves, splits, warp orchestration) are the next
//! tranche; they follow this same pattern.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::TrackKind;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{ArrangementSelection, ArrangementState, UiClip, UiNoteClip, UiTrack};

/// Messages the arrangement domain handles (track tranche).
#[derive(Debug, Clone)]
pub enum ArrangementMsg {
    AddTrack,
    AddMidiTrack,
    AddInstrumentTrack,
    RemoveTrack(TrackId),
    SelectTrack(TrackId),
    RenameTrack(TrackId, String),
    RenameClip(TrackId, ClipId, String),
    MoveTrackUp(TrackId),
    MoveTrackDown(TrackId),
    MoveSelectedTrackUp,
    MoveSelectedTrackDown,
    SetTrackGain(TrackId, f32),
    SetTrackPan(TrackId, f32),
    SetTrackMute(TrackId),
    SetTrackSolo(TrackId),
    EngineTrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },
    // ── Clip tranche ──
    RemoveClip(TrackId, ClipId),
    SelectArrangementClip {
        selection: ArrangementSelection,
        shift_held: bool,
    },
    MoveAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_position: u64,
    },
    MoveNoteClipPosition {
        track_id: TrackId,
        clip_id: ClipId,
        new_position_beats: f64,
    },
    ResizeAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_duration: u64,
    },
    MoveClipToTrack {
        source_track: TrackId,
        target_track: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    ToggleClipLoop(TrackId, ClipId),
    SetClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start: u64,
        loop_end: u64,
    },
    SetTimeSelection {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    SetTimeSelectionActive(bool),
    SetSelectionAsLoop,
    DeleteSelectedClip,
    DuplicateSelectedClip,
}

impl ArrangementMsg {
    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            ArrangementMsg::SelectTrack(_)
                | ArrangementMsg::EngineTrackMeter { .. }
                | ArrangementMsg::SelectArrangementClip { .. }
                | ArrangementMsg::SetTimeSelection { .. }
                | ArrangementMsg::SetTimeSelectionActive(_)
                | ArrangementMsg::SetSelectionAsLoop
        )
    }
}

/// Cross-domain effects requested by an arrangement update.
#[derive(Debug, Default, PartialEq)]
pub struct ArrangementAction {
    /// All plugin GUI windows and raw pointers of this track must go
    /// (the track's devices are being destroyed).
    pub close_track_guis: Option<TrackId>,
    /// Status bar text.
    pub status: Option<String>,
    /// Selecting a clip focuses the detail panel's Clip tab.
    pub focus_clip_tab: bool,
    /// A time selection was promoted to the transport loop region.
    pub loop_from_selection: Option<(f64, f64)>,
    /// A drag moved a clip near the view edge; auto-scroll to it.
    pub scroll_to_beat: Option<f64>,
}

/// Read-only cross-domain facts for arrangement updates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArrangementCtx {
    /// Samples per beat at the current tempo (clip drag snapping).
    pub samples_per_beat: f64,
}

impl ArrangementState {
    fn find_track(&self, track_id: TrackId) -> Option<&UiTrack> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

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

    fn find_track_mut(&mut self, track_id: TrackId) -> Option<&mut UiTrack> {
        self.tracks.iter_mut().find(|t| t.id == track_id)
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

    pub fn update(
        &mut self,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let _ = ctx.samples_per_beat; // used by clip arms below
        let mut action = ArrangementAction::default();
        match msg {
            ArrangementMsg::AddTrack => {
                let track_num = self.next_unique_track_number("Track");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");
                engine.send(EngineCommand::AddTrack(id, name.clone()));
                self.tracks.push(UiTrack::new(id, name, color_index));
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", self.tracks.len()));
            }
            ArrangementMsg::AddMidiTrack | ArrangementMsg::AddInstrumentTrack => {
                let track_num = self.next_unique_track_number("MIDI");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("MIDI {track_num}");
                engine.send(EngineCommand::AddMidiTrack(id, name.clone()));
                let mut track = UiTrack::new_instrument(id, name, TrackKind::Midi, color_index);
                track.has_instrument = false;
                self.tracks.push(track);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", self.tracks.len()));
            }
            ArrangementMsg::RemoveTrack(track_id) => {
                let removed_name = self
                    .tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("{track_id}"));

                engine.send(EngineCommand::RemoveTrack(track_id));
                self.tracks.retain(|t| t.id != track_id);
                if self.selected_track == Some(track_id) {
                    self.selected_track = self.tracks.first().map(|t| t.id);
                }
                if let Some((tid, _)) = self.selected_note_clip {
                    if tid == track_id {
                        self.selected_note_clip = None;
                    }
                }
                self.selected_clips.retain(|sel| {
                    let sel_track = match sel {
                        ArrangementSelection::AudioClip { track_id: t, .. } => *t,
                        ArrangementSelection::NoteClip { track_id: t, .. } => *t,
                    };
                    sel_track != track_id
                });
                action.close_track_guis = Some(track_id);
                action.status = Some(format!(
                    "Removed {removed_name}. {} track(s) remain.",
                    self.tracks.len()
                ));
            }
            ArrangementMsg::SelectTrack(track_id) => {
                self.selected_track = Some(track_id);
            }
            ArrangementMsg::RenameTrack(track_id, new_name) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.name = new_name;
                }
            }
            ArrangementMsg::RenameClip(track_id, clip_id, new_name) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    if let Some(c) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name.clone();
                    }
                    if let Some(c) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name;
                    }
                }
            }
            ArrangementMsg::MoveTrackUp(track_id) => self.move_track(track_id, true, engine),
            ArrangementMsg::MoveTrackDown(track_id) => self.move_track(track_id, false, engine),
            ArrangementMsg::MoveSelectedTrackUp => {
                if let Some(tid) = self.selected_track {
                    self.move_track(tid, true, engine);
                }
            }
            ArrangementMsg::MoveSelectedTrackDown => {
                if let Some(tid) = self.selected_track {
                    self.move_track(tid, false, engine);
                }
            }
            ArrangementMsg::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                engine.send(EngineCommand::SetTrackGain(track_id, gain));
                if let Some(track) = self.find_track_mut(track_id) {
                    track.gain = gain;
                }
            }
            ArrangementMsg::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                engine.send(EngineCommand::SetTrackPan(track_id, pan));
                if let Some(track) = self.find_track_mut(track_id) {
                    track.pan = pan;
                }
            }
            ArrangementMsg::SetTrackMute(track_id) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.mute = !track.mute;
                    let mute = track.mute;
                    engine.send(EngineCommand::SetTrackMute(track_id, mute));
                }
            }
            ArrangementMsg::SetTrackSolo(track_id) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.solo = !track.solo;
                    let solo = track.solo;
                    engine.send(EngineCommand::SetTrackSolo(track_id, solo));
                }
            }
            ArrangementMsg::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }
            ArrangementMsg::RemoveClip(track_id, clip_id) => {
                engine.send(EngineCommand::RemoveClip(track_id, clip_id));
                if let Some(track) = self.find_track_mut(track_id) {
                    track.clips.retain(|c| c.id != clip_id);
                }
                // Clear from multi-selection if this clip was selected
                self.selected_clips
                    .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
            }
            ArrangementMsg::ToggleClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.find_track_mut(track_id) {
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
                if let Some(track) = self.find_track_mut(track_id) {
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
                if let Some(track) = self.find_track_mut(track_id) {
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
                if let Some(track) = self.find_track_mut(track_id) {
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
                // Update UI state — auto-enable loop when extending past source length
                let spb = ctx.samples_per_beat;
                let mut sync_data = None;
                let mut clip_end_beat = None;
                if let Some(track) = self.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        let source_len = clip.audio.num_frames() as u64 - clip.source_offset;
                        if new_duration > source_len {
                            // Extending past source: enable loop over full source region
                            clip.duration = new_duration;
                            if !clip.loop_enabled {
                                clip.loop_enabled = true;
                                clip.loop_start = clip.source_offset;
                                clip.loop_end = clip.source_offset + source_len;
                            }
                        } else {
                            clip.duration = new_duration;
                        }
                        clip_end_beat = Some((clip.position + clip.duration) as f64 / spb);
                        sync_data = Some((
                            Arc::clone(&clip.audio),
                            clip.position,
                            clip.source_offset,
                            clip.duration,
                            clip.loop_enabled,
                            clip.loop_start,
                            clip.loop_end,
                        ));
                    }
                }
                // Sync to engine via Remove+Add (loop state included atomically)
                if let Some((
                    audio,
                    position,
                    source_offset,
                    duration,
                    loop_enabled,
                    loop_start,
                    loop_end,
                )) = sync_data
                {
                    engine.send(EngineCommand::RemoveClip(track_id, clip_id));
                    engine.send(EngineCommand::AddClip {
                        track_id,
                        clip_id,
                        audio,
                        position,
                        source_offset,
                        duration,
                        loop_enabled,
                        loop_start,
                        loop_end,
                    });
                }
                if let Some(end_beat) = clip_end_beat {
                    action.scroll_to_beat = Some(end_beat);
                }
                self.drag_resize_active = true;
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
                    if let Some(track) = self.find_track_mut(source_track) {
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
                        if let Some(track) = self.find_track_mut(target_track) {
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
                    if let Some(track) = self.find_track_mut(source_track) {
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
                        if let Some(track) = self.find_track_mut(target_track) {
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
                                if let Some(track) = self.find_track_mut(*track_id) {
                                    track.clips.retain(|c| c.id != *clip_id);
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                engine.send(EngineCommand::RemoveNoteClip(*track_id, *clip_id));
                                if let Some(track) = self.find_track_mut(*track_id) {
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
                                let mut dup_data = None;
                                if let Some(track) = self.find_track(*track_id) {
                                    if let Some(clip) =
                                        track.clips.iter().find(|c| c.id == *clip_id)
                                    {
                                        let new_pos = clip.position + clip.duration;
                                        dup_data = Some((
                                            Arc::clone(&clip.audio),
                                            clip.name.clone(),
                                            clip.source.clone(),
                                            new_pos,
                                            clip.source_offset,
                                            clip.duration,
                                        ));
                                    }
                                }
                                if let Some((
                                    audio,
                                    name,
                                    source,
                                    position,
                                    source_offset,
                                    duration,
                                )) = dup_data
                                {
                                    let new_id = ClipId::new();
                                    engine.send(EngineCommand::AddClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                        audio: Arc::clone(&audio),
                                        position,
                                        source_offset,
                                        duration,
                                        loop_enabled: false,
                                        loop_start: 0,
                                        loop_end: 0,
                                    });
                                    if let Some(track) = self.find_track_mut(*track_id) {
                                        track.clips.push(UiClip {
                                            id: new_id,
                                            name: format!("{name} (copy)"),
                                            audio,
                                            source,
                                            position,
                                            source_offset,
                                            duration,
                                            loop_enabled: false,
                                            loop_start: 0,
                                            loop_end: 0,
                                            original_bpm: None,
                                            warped: false,
                                            warped_to_bpm: None,
                                            original_audio: None,
                                        });
                                    }
                                    new_selections.insert(ArrangementSelection::AudioClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                    });
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                // Duplicate note clip inline
                                let mut dup_data = None;
                                if let Some(track) = self.find_track(*track_id) {
                                    if let Some(clip) =
                                        track.note_clips.iter().find(|c| c.id == *clip_id)
                                    {
                                        dup_data = Some((
                                            clip.name.clone(),
                                            clip.position_beats + clip.duration_beats,
                                            clip.duration_beats,
                                            clip.notes.clone(),
                                        ));
                                    }
                                }
                                if let Some((name, new_pos, dur, notes)) = dup_data {
                                    let new_id = ClipId::new();
                                    engine.send(EngineCommand::AddNoteClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                        position_beats: new_pos,
                                        duration_beats: dur,
                                        loop_enabled: false,
                                        loop_start_beats: 0.0,
                                        loop_end_beats: 0.0,
                                    });
                                    for note in &notes {
                                        engine.send(EngineCommand::AddNote {
                                            track_id: *track_id,
                                            clip_id: new_id,
                                            note: *note,
                                        });
                                    }
                                    if let Some(track) = self.find_track_mut(*track_id) {
                                        track.note_clips.push(UiNoteClip {
                                            id: new_id,
                                            name: format!("{name} (copy)"),
                                            position_beats: new_pos,
                                            duration_beats: dur,
                                            notes,
                                            selected_notes: HashSet::new(),
                                            loop_enabled: false,
                                            loop_start_beats: 0.0,
                                            loop_end_beats: 0.0,
                                        });
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
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    fn arrangement_with_tracks(n: usize) -> ArrangementState {
        let mut a = ArrangementState {
            next_track_number: 1,
            ..Default::default()
        };
        let mut engine = RecordingEngine::default();
        for _ in 0..n {
            a.update(
                ArrangementMsg::AddTrack,
                &mut engine,
                ArrangementCtx::default(),
            );
        }
        a
    }

    #[test]
    fn add_track_selects_it_and_names_uniquely() {
        let a = arrangement_with_tracks(2);
        assert_eq!(a.tracks.len(), 2);
        assert_eq!(a.tracks[1].name, "Track 2");
        assert_eq!(a.selected_track, Some(a.tracks[1].id));
    }

    #[test]
    fn remove_track_clears_its_selections_and_requests_gui_teardown() {
        let mut a = arrangement_with_tracks(2);
        let victim = a.tracks[1].id;
        let survivor = a.tracks[0].id;
        a.selected_note_clip = Some((victim, ClipId::new()));
        let mut engine = RecordingEngine::default();
        let action = a.update(
            ArrangementMsg::RemoveTrack(victim),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks.len(), 1);
        assert_eq!(a.selected_track, Some(survivor));
        assert_eq!(a.selected_note_clip, None);
        assert_eq!(action.close_track_guis, Some(victim));
    }

    #[test]
    fn reorder_sends_full_order_and_respects_bounds() {
        let mut a = arrangement_with_tracks(2);
        let first = a.tracks[0].id;
        let mut engine = RecordingEngine::default();
        // Top track cannot move further up: no command.
        a.update(
            ArrangementMsg::MoveTrackUp(first),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert!(engine.0.is_empty());
        a.update(
            ArrangementMsg::MoveTrackDown(first),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[1].id, first);
        assert!(matches!(engine.0[0], EngineCommand::ReorderTracks(_)));
    }

    #[test]
    fn gain_and_pan_clamp() {
        let mut a = arrangement_with_tracks(1);
        let id = a.tracks[0].id;
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::SetTrackGain(id, 99.0),
            &mut engine,
            ArrangementCtx::default(),
        );
        a.update(
            ArrangementMsg::SetTrackPan(id, -5.0),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].gain, 2.0);
        assert_eq!(a.tracks[0].pan, 0.0);
    }

    #[test]
    fn meter_decays_instead_of_snapping() {
        let mut a = arrangement_with_tracks(1);
        let id = a.tracks[0].id;
        a.tracks[0].peak_l = 1.0;
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::EngineTrackMeter {
                track_id: id,
                peak_l: 0.0,
                peak_r: 0.0,
            },
            &mut engine,
            ArrangementCtx::default(),
        );
        assert!((a.tracks[0].peak_l - 0.85).abs() < 1e-6);
    }
}
