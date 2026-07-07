//! Arrangement domain, tranche one: track lifecycle and mixing
//! basics. Owns the track list (the shared model other domains
//! receive explicitly), selection, and track numbering.
//!
//! Clip operations (moves, splits, warp orchestration) are the next
//! tranche; they follow this same pattern.

use std::collections::HashSet;

use crate::message::{AudioQuantizeSuccess, AutoWarpOutcome, ClipWarpSuccess};
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};
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
    DuplicateNoteClip(TrackId, ClipId),
    SplitAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_position: u64,
    },
    SplitNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    },
    SplitSelectedAtPlayhead,
    JoinSelectedClips,
    DeleteClipsInRegion {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    SplitClipsAtRegion {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    },
    CreateClipFromSelection,
    CreateNoteClipFromSelection(TrackId),
    ClipBpmInputChanged {
        track_id: TrackId,
        clip_id: ClipId,
        text: String,
    },
    SubmitClipBpm {
        track_id: TrackId,
        clip_id: ClipId,
    },
    SetClipNominalBpm {
        track_id: TrackId,
        clip_id: ClipId,
        bpm: f64,
    },
    ClearClipWarp {
        track_id: TrackId,
        clip_id: ClipId,
    },
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
                | ArrangementMsg::ClipBpmInputChanged { .. }
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
    /// Dismiss the arrangement context menu.
    pub close_context_menu: bool,
    /// The project content changed outside the undo-snapshot path.
    pub mark_dirty: bool,
}

/// Read-only cross-domain facts for arrangement updates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArrangementCtx {
    /// Samples per beat at the current tempo (clip drag snapping).
    pub samples_per_beat: f64,
    /// Playhead position in samples (split-at-playhead).
    pub playhead_samples: u64,
    /// Playhead position in beats.
    pub playhead_beats: f64,
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
            ArrangementMsg::DuplicateNoteClip(track_id, clip_id) => {
                let new_clip_id = ClipId::new();
                let mut new_clip_data = None;

                if let Some(track) = self.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let new_pos = clip.position_beats + clip.duration_beats;
                        new_clip_data = Some((
                            UiNoteClip {
                                id: new_clip_id,
                                name: format!("{} (copy)", clip.name),
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
                        ));
                    }
                }

                if let Some((new_clip, pos, dur, notes)) = new_clip_data {
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.note_clips.push(new_clip);
                    }
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: new_clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
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
                let mut split_data = None;
                if let Some(track) = self.find_track(track_id) {
                    if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                        if split_position > clip.position
                            && split_position < clip.position + clip.duration
                        {
                            let left_dur = split_position - clip.position;
                            let right_dur = clip.duration - left_dur;
                            let right_source_offset = clip.source_offset + left_dur;
                            split_data = Some((
                                Arc::clone(&clip.audio),
                                clip.name.clone(),
                                clip.source.clone(),
                                clip.position,
                                clip.source_offset,
                                left_dur,
                                split_position,
                                right_source_offset,
                                right_dur,
                            ));
                        }
                    }
                }
                if let Some((
                    audio,
                    name,
                    source,
                    orig_pos,
                    orig_offset,
                    left_dur,
                    right_pos,
                    right_offset,
                    right_dur,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    engine.send(EngineCommand::RemoveClip(track_id, clip_id));
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    engine.send(EngineCommand::AddClip {
                        track_id,
                        clip_id: left_id,
                        audio: Arc::clone(&audio),
                        position: orig_pos,
                        source_offset: orig_offset,
                        duration: left_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: left_id,
                            name: format!("{name} L"),
                            audio: Arc::clone(&audio),
                            source: source.clone(),
                            position: orig_pos,
                            source_offset: orig_offset,
                            duration: left_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            original_bpm: None,
                            warped: false,
                            warped_to_bpm: None,
                            original_audio: None,
                        });
                    }

                    // Add right half
                    engine.send(EngineCommand::AddClip {
                        track_id,
                        clip_id: right_id,
                        audio: Arc::clone(&audio),
                        position: right_pos,
                        source_offset: right_offset,
                        duration: right_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: right_id,
                            name: format!("{name} R"),
                            audio,
                            source,
                            position: right_pos,
                            source_offset: right_offset,
                            duration: right_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            original_bpm: None,
                            warped: false,
                            warped_to_bpm: None,
                            original_audio: None,
                        });
                    }

                    // Update selection: remove original, add left half
                    self.selected_clips
                        .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
                    self.selected_clips.insert(ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: left_id,
                    });
                    action.status = Some("Split audio clip".to_string());
                }
            }
            ArrangementMsg::SplitNoteClip {
                track_id,
                clip_id,
                split_beat,
            } => {
                let mut split_data = None;
                if let Some(track) = self.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let clip_end = clip.position_beats + clip.duration_beats;
                        if split_beat > clip.position_beats && split_beat < clip_end {
                            let local_split = split_beat - clip.position_beats;
                            let left_dur = local_split;
                            let right_dur = clip.duration_beats - local_split;

                            let mut left_notes = Vec::new();
                            let mut right_notes = Vec::new();
                            for note in &clip.notes {
                                if note.start_beat < local_split {
                                    left_notes.push(*note);
                                } else {
                                    right_notes.push(MidiNote {
                                        start_beat: note.start_beat - local_split,
                                        ..*note
                                    });
                                }
                            }

                            split_data = Some((
                                clip.name.clone(),
                                clip.position_beats,
                                left_dur,
                                split_beat,
                                right_dur,
                                left_notes,
                                right_notes,
                            ));
                        }
                    }
                }
                if let Some((
                    name,
                    orig_pos,
                    left_dur,
                    right_pos,
                    right_dur,
                    left_notes,
                    right_notes,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    engine.send(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.note_clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: left_id,
                        position_beats: orig_pos,
                        duration_beats: left_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &left_notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id: left_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: left_id,
                            name: format!("{name} L"),
                            position_beats: orig_pos,
                            duration_beats: left_dur,
                            notes: left_notes,
                            selected_notes: HashSet::new(),
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    // Add right half
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: right_id,
                        position_beats: right_pos,
                        duration_beats: right_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &right_notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id: right_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: right_id,
                            name: format!("{name} R"),
                            position_beats: right_pos,
                            duration_beats: right_dur,
                            notes: right_notes,
                            selected_notes: HashSet::new(),
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    // Update selection: remove original, add left half
                    self.selected_clips
                        .remove(&ArrangementSelection::NoteClip { track_id, clip_id });
                    self.selected_clips.insert(ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: left_id,
                    });
                    self.selected_note_clip = Some((track_id, left_id));
                    action.status = Some("Split note clip".to_string());
                }
            }
            ArrangementMsg::SplitSelectedAtPlayhead => {
                if self.time_selection_active
                    && self.selection_end_beats > self.selection_start_beats
                {
                    return self.update(
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
                let clips: Vec<_> = self.selected_clips.iter().copied().collect();
                if clips.len() < 2 {
                    return action;
                }

                // Validate: all must be same type and same track
                let first_track = match clips[0] {
                    ArrangementSelection::AudioClip { track_id, .. } => track_id,
                    ArrangementSelection::NoteClip { track_id, .. } => track_id,
                };
                let all_audio = clips.iter().all(|s| {
                    matches!(s, ArrangementSelection::AudioClip { track_id, .. } if *track_id == first_track)
                });
                let all_note = clips.iter().all(|s| {
                    matches!(s, ArrangementSelection::NoteClip { track_id, .. } if *track_id == first_track)
                });

                if all_audio {
                    action.status = self.join_audio_clips(first_track, &clips, engine);
                } else if all_note {
                    action.status = self.join_note_clips(first_track, &clips, engine);
                } else {
                    action.status = Some("Join requires same type and track".to_string());
                }
            }
            ArrangementMsg::DeleteClipsInRegion {
                start_beats,
                end_beats,
                track_id: target_track,
            } => {
                action.close_context_menu = true;
                let spb = ctx.samples_per_beat;
                // Collect clip IDs to remove
                let mut audio_removals: Vec<(TrackId, ClipId)> = Vec::new();
                let mut note_removals: Vec<(TrackId, ClipId)> = Vec::new();
                for track in &self.tracks {
                    if let Some(tid) = target_track {
                        if track.id != tid {
                            continue;
                        }
                    }
                    if spb > 0.0 {
                        for clip in &track.clips {
                            let clip_start = clip.position as f64 / spb;
                            let clip_end = (clip.position + clip.duration) as f64 / spb;
                            if clip_start < end_beats && clip_end > start_beats {
                                audio_removals.push((track.id, clip.id));
                            }
                        }
                    }
                    for nc in &track.note_clips {
                        let clip_end = nc.position_beats + nc.duration_beats;
                        if nc.position_beats < end_beats && clip_end > start_beats {
                            note_removals.push((track.id, nc.id));
                        }
                    }
                }
                for (tid, cid) in &audio_removals {
                    engine.send(EngineCommand::RemoveClip(*tid, *cid));
                    if let Some(track) = self.find_track_mut(*tid) {
                        track.clips.retain(|c| c.id != *cid);
                    }
                }
                for (tid, cid) in &note_removals {
                    engine.send(EngineCommand::RemoveNoteClip(*tid, *cid));
                    if let Some(track) = self.find_track_mut(*tid) {
                        track.note_clips.retain(|c| c.id != *cid);
                    }
                }
                self.selected_clips.clear();
                self.selected_note_clip = None;
                self.time_selection_active = false;
                let count = audio_removals.len() + note_removals.len();
                action.status = Some(format!("Deleted {count} clips in region"));
            }
            ArrangementMsg::SplitClipsAtRegion {
                start_beats,
                end_beats,
                track_id: target_track,
            } => {
                action.close_context_menu = true;
                let spb = ctx.samples_per_beat;
                let mut split_count = 0u32;

                // Split at start boundary first, then end boundary.
                // After a split, new clips replace the original, so we
                // re-scan the track list between boundary passes.
                for boundary_beats in [start_beats, end_beats] {
                    let boundary_sample = (boundary_beats * spb) as u64;

                    // Collect audio splits for this boundary, limited to
                    // the originating track when the selection was drawn
                    // on a single lane.
                    let audio_hits: Vec<(TrackId, ClipId)> = if spb > 0.0 {
                        self.tracks
                            .iter()
                            .filter(|t| target_track.is_none_or(|tid| t.id == tid))
                            .flat_map(|t| {
                                t.clips.iter().filter_map(|c| {
                                    let cs = c.position as f64 / spb;
                                    let ce = (c.position + c.duration) as f64 / spb;
                                    if boundary_beats > cs && boundary_beats < ce {
                                        Some((t.id, c.id))
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let note_hits: Vec<(TrackId, ClipId)> = self
                        .tracks
                        .iter()
                        .filter(|t| target_track.is_none_or(|tid| t.id == tid))
                        .flat_map(|t| {
                            t.note_clips.iter().filter_map(|c| {
                                let ce = c.position_beats + c.duration_beats;
                                if boundary_beats > c.position_beats && boundary_beats < ce {
                                    Some((t.id, c.id))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    for (tid, cid) in audio_hits {
                        let _ = self.update(
                            ArrangementMsg::SplitAudioClip {
                                track_id: tid,
                                clip_id: cid,
                                split_position: boundary_sample,
                            },
                            engine,
                            ctx,
                        );
                        split_count += 1;
                    }
                    for (tid, cid) in note_hits {
                        let _ = self.update(
                            ArrangementMsg::SplitNoteClip {
                                track_id: tid,
                                clip_id: cid,
                                split_beat: boundary_beats,
                            },
                            engine,
                            ctx,
                        );
                        split_count += 1;
                    }
                }

                if split_count > 0 {
                    action.status = Some(format!("Split {split_count} clips at region boundaries"));
                }
            }
            ArrangementMsg::CreateClipFromSelection => {
                if let Some(tid) = self.selected_track {
                    if let Some(track) = self.find_track(tid) {
                        if track.kind.is_midi() {
                            return self.update(
                                ArrangementMsg::CreateNoteClipFromSelection(tid),
                                engine,
                                ctx,
                            );
                        } else {
                            action.status =
                                Some("Select a time region on a MIDI track".to_string());
                        }
                    }
                } else {
                    action.status = Some("No track selected".to_string());
                }
            }
            ArrangementMsg::CreateNoteClipFromSelection(track_id) => {
                action.close_context_menu = true;
                if !self.time_selection_active
                    || self.selection_end_beats <= self.selection_start_beats
                {
                    action.status = Some("No time selection active".to_string());
                    return action;
                }
                if let Some(track) = self.find_track(track_id) {
                    if !track.kind.is_midi() {
                        action.status =
                            Some("Can only create note clips on MIDI tracks".to_string());
                        return action;
                    }
                }
                let clip_id = ClipId::new();
                let position_beats = self.selection_start_beats;
                let duration_beats = self.selection_end_beats - self.selection_start_beats;
                if let Some(track) = self.find_track_mut(track_id) {
                    track.note_clips.push(UiNoteClip {
                        id: clip_id,
                        name: format!("Pattern {}", track.note_clips.len() + 1),
                        position_beats,
                        duration_beats,
                        notes: Vec::new(),
                        selected_notes: HashSet::new(),
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                }
                engine.send(EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                });
                self.selected_note_clip = Some((track_id, clip_id));
                self.selected_clips.clear();
                self.selected_clips
                    .insert(ArrangementSelection::NoteClip { track_id, clip_id });
                action.status = Some("Created note clip from selection".to_string());
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
                    if let Some(track) = self.find_track_mut(track_id) {
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
                if let Some(track) = self.find_track_mut(track_id) {
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
        }
        action
    }

    /// A background warp finished: swap in the stretched audio and
    /// record the warp geometry on the clip.
    pub fn apply_clip_warp_success(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        success: ClipWarpSuccess,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        engine.send(EngineCommand::ReplaceClipAudio {
            track_id,
            clip_id,
            audio: Arc::clone(&success.audio),
            duration: success.new_duration,
            source_offset: success.new_source_offset,
            loop_start: success.new_loop_start,
            loop_end: success.new_loop_end,
        });
        if let Some(track) = self.find_track_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                clip.audio = Arc::clone(&success.audio);
                clip.duration = success.new_duration;
                clip.source_offset = success.new_source_offset;
                clip.loop_start = success.new_loop_start;
                clip.loop_end = success.new_loop_end;
                clip.original_bpm = Some(success.detected_bpm);
                clip.warped = true;
                clip.warped_to_bpm = Some(success.warped_to_bpm);
                clip.original_audio = Some(Arc::clone(&success.original_audio));
            }
        }
        action.status = Some(format!("Warped to {:.0} BPM", success.warped_to_bpm));
        action.mark_dirty = true;
        action
    }

    /// An auto-warp-on-import attempt finished.
    pub fn apply_auto_warp_outcome(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        outcome: AutoWarpOutcome,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        match outcome {
            AutoWarpOutcome::NotDetected => {
                // Nothing to apply. Point the user at the manual
                // workflow in the clip detail panel.
                action.status = Some(
                    "Auto-warp: could not detect BPM. Select the clip and type the source \
                     BPM in the Warp row, then press Enter and click Warp."
                        .to_string(),
                );
            }
            AutoWarpOutcome::DetectedOnly { bpm, confidence } => {
                if let Some(track) = self.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(bpm);
                    }
                }
                action.status = Some(format!(
                    "Auto-warp skipped: detected {:.1} BPM at low confidence {:.2}. \
                     Use the clip's Warp button to apply it manually.",
                    bpm, confidence
                ));
                action.mark_dirty = true;
            }
            AutoWarpOutcome::Warped { success, .. } => {
                return self.apply_clip_warp_success(engine, track_id, clip_id, success);
            }
        }
        action
    }

    /// Restore a warped clip's original audio (or just drop the warp
    /// flags when the original is gone).
    pub fn apply_clear_clip_warp(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let restore = self
            .find_track(track_id)
            .and_then(|track| track.clips.iter().find(|c| c.id == clip_id))
            .and_then(|clip| clip.original_audio.as_ref().map(Arc::clone));
        if let Some(original) = restore {
            let original_frames = original.num_frames() as u64;
            engine.send(EngineCommand::ReplaceClipAudio {
                track_id,
                clip_id,
                audio: Arc::clone(&original),
                duration: original_frames,
                source_offset: 0,
                loop_start: 0,
                loop_end: 0,
            });
            if let Some(track) = self.find_track_mut(track_id) {
                if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                    clip.audio = original;
                    clip.duration = original_frames;
                    clip.source_offset = 0;
                    clip.loop_start = 0;
                    clip.loop_end = 0;
                    clip.warped = false;
                    clip.warped_to_bpm = None;
                    clip.original_audio = None;
                }
            }
        } else if let Some(track) = self.find_track_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                clip.warped = false;
                clip.warped_to_bpm = None;
            }
        }
        action.status = Some("Cleared clip warp".to_string());
        action.mark_dirty = true;
        action
    }

    /// A background audio quantize finished: replace the source clip
    /// with the newly rendered one and select it.
    pub fn apply_audio_quantize_success(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        old_clip_id: ClipId,
        success: AudioQuantizeSuccess,
        sample_rate: u32,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        engine.send(EngineCommand::RemoveClip(track_id, old_clip_id));
        if let Some(track) = self.find_track_mut(track_id) {
            track.clips.retain(|c| c.id != old_clip_id);
        }
        self.selected_clips.retain(|sel| match sel {
            ArrangementSelection::AudioClip {
                clip_id: cid,
                track_id: tid,
            } => !(*tid == track_id && *cid == old_clip_id),
            _ => true,
        });

        engine.send(EngineCommand::AddClip {
            track_id,
            clip_id: success.new_clip_id,
            audio: Arc::clone(&success.new_audio),
            position: success.new_position,
            source_offset: 0,
            duration: success.new_duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        if let Some(track) = self.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: success.new_clip_id,
                name: success.new_name,
                audio: Arc::clone(&success.new_audio),
                source: None,
                position: success.new_position,
                source_offset: 0,
                duration: success.new_duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }
        self.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id,
            clip_id: success.new_clip_id,
        });

        let duration_seconds = success.new_duration as f64 / sample_rate.max(1) as f64;
        action.status = Some(format!(
            "Quantized {} slice(s) to {} ({:.1}s)",
            success.slice_count, success.grid_label, duration_seconds
        ));
        action
    }

    /// A background BPM detection finished.
    pub fn apply_clip_bpm_detected(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        bpm: Option<f64>,
        confidence: f32,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        match bpm {
            Some(b) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(b);
                    }
                }
                self.clip_bpm_edit.remove(&clip_id);
                action.status = Some(format!(
                    "Detected {:.1} BPM (confidence {:.2})",
                    b, confidence
                ));
                action.mark_dirty = true;
            }
            None => {
                action.status = Some(
                    "Could not detect BPM. Type the source BPM in the Warp row and \
                     press Enter, then click Warp."
                        .to_string(),
                );
            }
        }
        action
    }

    fn join_audio_clips(
        &mut self,
        track_id: TrackId,
        selections: &[ArrangementSelection],
        engine: &mut impl EngineHandle,
    ) -> Option<String> {
        // Collect clip data sorted by position
        let clip_ids: Vec<ClipId> = selections
            .iter()
            .filter_map(|s| match s {
                ArrangementSelection::AudioClip { clip_id, .. } => Some(*clip_id),
                _ => None,
            })
            .collect();

        let mut clip_data: Vec<(u64, u64, u64, Arc<vibez_core::audio_buffer::DecodedAudio>)> =
            Vec::new();
        if let Some(track) = self.find_track(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.clips.iter().find(|c| c.id == *cid) {
                    clip_data.push((
                        clip.position,
                        clip.source_offset,
                        clip.duration,
                        Arc::clone(&clip.audio),
                    ));
                }
            }
        }

        if clip_data.len() < 2 {
            return None;
        }

        // Sort by position
        clip_data.sort_by_key(|(pos, _, _, _)| *pos);

        let start_pos = clip_data[0].0;
        let end_pos = clip_data
            .iter()
            .map(|(pos, _, dur, _)| pos + dur)
            .max()
            .unwrap_or(start_pos);
        let total_duration = end_pos - start_pos;

        // Determine channel count from first clip
        let channels = clip_data[0].3.num_channels();
        let sr = clip_data[0].3.sample_rate;

        // Create joined buffer filled with silence
        let mut joined_channels: Vec<Vec<f32>> = (0..channels)
            .map(|_| vec![0.0f32; total_duration as usize])
            .collect();

        // Copy each clip's audio into the correct offset
        for (pos, source_offset, duration, audio) in &clip_data {
            let offset_in_joined = (*pos - start_pos) as usize;
            let src_off = *source_offset as usize;
            let dur = *duration as usize;
            let ch_count = channels.min(audio.num_channels());
            for (ch, dst) in joined_channels.iter_mut().enumerate().take(ch_count) {
                let src = &audio.channels[ch];
                let src_end = (src_off + dur).min(src.len());
                let copy_len = src_end.saturating_sub(src_off);
                let dst_end = (offset_in_joined + copy_len).min(dst.len());
                let actual_len = dst_end.saturating_sub(offset_in_joined);
                if actual_len > 0 {
                    dst[offset_in_joined..offset_in_joined + actual_len]
                        .copy_from_slice(&src[src_off..src_off + actual_len]);
                }
            }
        }

        // Create DecodedAudio
        let joined_audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: joined_channels,
            sample_rate: sr,
        });

        // Remove all originals
        for cid in &clip_ids {
            engine.send(EngineCommand::RemoveClip(track_id, *cid));
            if let Some(track) = self.find_track_mut(track_id) {
                track.clips.retain(|c| c.id != *cid);
            }
        }

        // Add joined clip
        let new_id = ClipId::new();
        engine.send(EngineCommand::AddClip {
            track_id,
            clip_id: new_id,
            audio: Arc::clone(&joined_audio),
            position: start_pos,
            source_offset: 0,
            duration: total_duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        if let Some(track) = self.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: new_id,
                name: "Joined".to_string(),
                audio: joined_audio,
                source: None,
                position: start_pos,
                source_offset: 0,
                duration: total_duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }

        self.selected_clips.clear();
        self.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id,
            clip_id: new_id,
        });
        Some("Joined audio clips".to_string())
    }

    fn join_note_clips(
        &mut self,
        track_id: TrackId,
        selections: &[ArrangementSelection],
        engine: &mut impl EngineHandle,
    ) -> Option<String> {
        let clip_ids: Vec<ClipId> = selections
            .iter()
            .filter_map(|s| match s {
                ArrangementSelection::NoteClip { clip_id, .. } => Some(*clip_id),
                _ => None,
            })
            .collect();

        let mut clip_data: Vec<(f64, f64, Vec<MidiNote>)> = Vec::new();
        if let Some(track) = self.find_track(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.note_clips.iter().find(|c| c.id == *cid) {
                    clip_data.push((clip.position_beats, clip.duration_beats, clip.notes.clone()));
                }
            }
        }

        if clip_data.len() < 2 {
            return None;
        }

        // Sort by position
        clip_data.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let start_pos = clip_data[0].0;
        let end_pos = clip_data
            .iter()
            .map(|(pos, dur, _)| pos + dur)
            .fold(0.0_f64, f64::max);
        let total_duration = end_pos - start_pos;

        // Merge notes with adjusted offsets
        let mut merged_notes: Vec<MidiNote> = Vec::new();
        for (pos, _, notes) in &clip_data {
            let offset = pos - start_pos;
            for note in notes {
                merged_notes.push(MidiNote {
                    start_beat: note.start_beat + offset,
                    ..*note
                });
            }
        }

        // Remove all originals
        for cid in &clip_ids {
            engine.send(EngineCommand::RemoveNoteClip(track_id, *cid));
            if let Some(track) = self.find_track_mut(track_id) {
                track.note_clips.retain(|c| c.id != *cid);
            }
        }

        // Add joined clip
        let new_id = ClipId::new();
        engine.send(EngineCommand::AddNoteClip {
            track_id,
            clip_id: new_id,
            position_beats: start_pos,
            duration_beats: total_duration,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        });
        for note in &merged_notes {
            engine.send(EngineCommand::AddNote {
                track_id,
                clip_id: new_id,
                note: *note,
            });
        }
        if let Some(track) = self.find_track_mut(track_id) {
            track.note_clips.push(UiNoteClip {
                id: new_id,
                name: "Joined".to_string(),
                position_beats: start_pos,
                duration_beats: total_duration,
                notes: merged_notes,
                selected_notes: HashSet::new(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            });
        }

        self.selected_clips.clear();
        self.selected_clips.insert(ArrangementSelection::NoteClip {
            track_id,
            clip_id: new_id,
        });
        self.selected_note_clip = Some((track_id, new_id));
        Some("Joined note clips".to_string())
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

    fn add_audio_clip(
        a: &mut ArrangementState,
        track_idx: usize,
        position: u64,
        duration: u64,
    ) -> (TrackId, ClipId) {
        let audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.0; (position + duration) as usize]],
            sample_rate: 44100,
        });
        let id = ClipId::new();
        let tid = a.tracks[track_idx].id;
        a.tracks[track_idx].clips.push(UiClip {
            id,
            name: "Clip".to_string(),
            audio,
            source: None,
            position,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
            original_audio: None,
        });
        (tid, id)
    }

    #[test]
    fn split_audio_clip_replaces_clip_with_two_halves() {
        let mut a = arrangement_with_tracks(1);
        let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::SplitAudioClip {
                track_id: tid,
                clip_id: cid,
                split_position: 400,
            },
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].clips.len(), 2);
        assert_eq!(a.tracks[0].clips[0].duration, 400);
        assert_eq!(a.tracks[0].clips[1].duration, 600);
        assert_eq!(a.tracks[0].clips[1].position, 400);
        assert_eq!(a.tracks[0].clips[1].source_offset, 400);
        assert!(matches!(engine.0[0], EngineCommand::RemoveClip(..)));
    }

    #[test]
    fn split_outside_clip_bounds_is_a_noop() {
        let mut a = arrangement_with_tracks(1);
        let (tid, cid) = add_audio_clip(&mut a, 0, 100, 500);
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::SplitAudioClip {
                track_id: tid,
                clip_id: cid,
                split_position: 50,
            },
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].clips.len(), 1);
        assert!(engine.0.is_empty());
    }

    #[test]
    fn join_merges_adjacent_audio_clips_into_one() {
        let mut a = arrangement_with_tracks(1);
        let (tid, c1) = add_audio_clip(&mut a, 0, 0, 100);
        let (_, c2) = add_audio_clip(&mut a, 0, 200, 100);
        a.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id: c1,
        });
        a.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id: c2,
        });
        let mut engine = RecordingEngine::default();
        let action = a.update(
            ArrangementMsg::JoinSelectedClips,
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].clips.len(), 1);
        assert_eq!(a.tracks[0].clips[0].position, 0);
        assert_eq!(a.tracks[0].clips[0].duration, 300);
        assert_eq!(action.status.as_deref(), Some("Joined audio clips"));
    }

    #[test]
    fn join_rejects_mixed_selection_types() {
        let mut a = arrangement_with_tracks(1);
        let (tid, c1) = add_audio_clip(&mut a, 0, 0, 100);
        a.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id: tid,
            clip_id: c1,
        });
        a.selected_clips.insert(ArrangementSelection::NoteClip {
            track_id: tid,
            clip_id: ClipId::new(),
        });
        let mut engine = RecordingEngine::default();
        let action = a.update(
            ArrangementMsg::JoinSelectedClips,
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].clips.len(), 1);
        assert_eq!(
            action.status.as_deref(),
            Some("Join requires same type and track")
        );
    }

    #[test]
    fn create_note_clip_needs_midi_track_and_active_selection() {
        let mut a = arrangement_with_tracks(1);
        let audio_tid = a.tracks[0].id;
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::AddMidiTrack,
            &mut engine,
            ArrangementCtx::default(),
        );
        let midi_tid = a.tracks[1].id;

        // No selection yet: refused.
        let action = a.update(
            ArrangementMsg::CreateNoteClipFromSelection(midi_tid),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(action.status.as_deref(), Some("No time selection active"));

        a.time_selection_active = true;
        a.selection_start_beats = 4.0;
        a.selection_end_beats = 8.0;

        // Audio track: refused.
        let action = a.update(
            ArrangementMsg::CreateNoteClipFromSelection(audio_tid),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(
            action.status.as_deref(),
            Some("Can only create note clips on MIDI tracks")
        );

        // MIDI track: creates and selects the clip.
        let action = a.update(
            ArrangementMsg::CreateNoteClipFromSelection(midi_tid),
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(
            action.status.as_deref(),
            Some("Created note clip from selection")
        );
        let clip = &a.tracks[1].note_clips[0];
        assert_eq!(clip.position_beats, 4.0);
        assert_eq!(clip.duration_beats, 4.0);
        assert_eq!(a.selected_note_clip, Some((midi_tid, clip.id)));
    }

    fn warp_success(
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    ) -> crate::message::ClipWarpSuccess {
        crate::message::ClipWarpSuccess {
            original_audio: Arc::clone(&audio),
            audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![vec![0.0; 2000]],
                sample_rate: 44100,
            }),
            new_duration: 2000,
            new_source_offset: 0,
            new_loop_start: 0,
            new_loop_end: 0,
            detected_bpm: 128.0,
            warped_to_bpm: 120.0,
        }
    }

    #[test]
    fn warp_then_clear_roundtrips_clip_geometry() {
        let mut a = arrangement_with_tracks(1);
        let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
        let original = Arc::clone(&a.tracks[0].clips[0].audio);
        let mut engine = RecordingEngine::default();

        let action =
            a.apply_clip_warp_success(&mut engine, tid, cid, warp_success(Arc::clone(&original)));
        let clip = &a.tracks[0].clips[0];
        assert!(clip.warped);
        assert_eq!(clip.duration, 2000);
        assert_eq!(clip.warped_to_bpm, Some(120.0));
        assert_eq!(clip.original_bpm, Some(128.0));
        assert!(clip.original_audio.is_some());
        assert!(action.mark_dirty);
        assert!(matches!(
            engine.0[0],
            EngineCommand::ReplaceClipAudio { .. }
        ));

        let action = a.apply_clear_clip_warp(&mut engine, tid, cid);
        let clip = &a.tracks[0].clips[0];
        assert!(!clip.warped);
        assert_eq!(clip.duration, 1000);
        assert!(clip.original_audio.is_none());
        assert!(Arc::ptr_eq(&clip.audio, &original));
        assert!(action.mark_dirty);
    }

    #[test]
    fn bpm_detected_commits_and_clears_pending_edit() {
        let mut a = arrangement_with_tracks(1);
        let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
        a.clip_bpm_edit.insert(cid, "999".to_string());
        let action = a.apply_clip_bpm_detected(tid, cid, Some(174.0), 0.9);
        assert_eq!(a.tracks[0].clips[0].original_bpm, Some(174.0));
        assert!(a.clip_bpm_edit.is_empty());
        assert!(action.mark_dirty);

        let action = a.apply_clip_bpm_detected(tid, cid, None, 0.0);
        assert!(!action.mark_dirty);
        assert!(action.status.unwrap().contains("Could not detect BPM"));
    }

    #[test]
    fn submit_clip_bpm_parses_and_rejects_garbage() {
        let mut a = arrangement_with_tracks(1);
        let (tid, cid) = add_audio_clip(&mut a, 0, 0, 1000);
        let mut engine = RecordingEngine::default();
        a.clip_bpm_edit.insert(cid, "140.5".to_string());
        let action = a.update(
            ArrangementMsg::SubmitClipBpm {
                track_id: tid,
                clip_id: cid,
            },
            &mut engine,
            ArrangementCtx::default(),
        );
        assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
        assert!(action.mark_dirty);

        a.clip_bpm_edit.insert(cid, "not a number".to_string());
        let action = a.update(
            ArrangementMsg::SubmitClipBpm {
                track_id: tid,
                clip_id: cid,
            },
            &mut engine,
            ArrangementCtx::default(),
        );
        assert!(!action.mark_dirty);
        assert_eq!(a.tracks[0].clips[0].original_bpm, Some(140.5));
    }
}
