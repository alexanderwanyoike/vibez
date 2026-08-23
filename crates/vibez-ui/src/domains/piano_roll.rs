//! Piano roll domain: note clip CRUD, note editing/selection, clip
//! loop regions, and the edit-mode toggle.
//!
//! Same shape as the devices domain: `update` receives the piano
//! roll slice, an engine handle, the shared track list, and a small
//! read-only context; cross-domain effects come back as a
//! [`PianoRollAction`] for app.rs to route.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{
    PianoRollState, SnapGrid, TimelineContent, TimelineEditorState, TrackTimelineContent,
    UiNoteClip,
};

/// Messages the piano roll domain handles.
#[derive(Debug, Clone)]
pub enum PianoRollMsg {
    ToggleNoteClipLoop(TrackId, ClipId),
    SetNoteClipGrooveGrid(TrackId, ClipId, GrooveGrid),
    SetNoteClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start_beats: f64,
        loop_end_beats: f64,
    },
    AddNoteClipToTrack(TrackId),
    SelectNoteClip(TrackId, ClipId),
    AddNote {
        track_id: TrackId,
        clip_id: ClipId,
        pitch: u8,
        start_beat: f64,
        duration_beats: f64,
    },
    RemoveNote(TrackId, ClipId, usize),
    EditNote(TrackId, ClipId, usize, MidiNote),
    /// Set attack Velocity for one or more MIDI Notes as one project edit.
    /// Invalid note indices are ignored and values are clamped to `1..127`.
    SetNoteVelocities {
        track_id: TrackId,
        clip_id: ClipId,
        velocities: Vec<(usize, u8)>,
    },
    SelectNote(TrackId, ClipId, Option<usize>, bool),
    SelectAllNotes(TrackId, ClipId),
    /// Rubber-band selection: catch every note overlapping the beat span
    /// and the inclusive pitch range the box covers.
    SelectNotesInRegion {
        track_id: TrackId,
        clip_id: ClipId,
        start_beat: f64,
        end_beat: f64,
        low_pitch: u8,
        high_pitch: u8,
        additive: bool,
    },
    RemoveSelectedNotes(TrackId, ClipId),
    NudgeSelectedNotes {
        track_id: TrackId,
        clip_id: ClipId,
        delta_beats: f64,
        delta_semitones: i8,
    },
    /// Batch-move notes to absolute positions (multi-note drag release).
    MoveNotesAbsolute {
        track_id: TrackId,
        clip_id: ClipId,
        /// (note_index, new_start_beat, new_pitch)
        moves: Vec<(usize, f64, u8)>,
    },
    DoubleNoteClip(TrackId, ClipId),
    CropNoteClip(TrackId, ClipId),
    ScrollY(f32),
    ResizeNoteClipDuration {
        track_id: TrackId,
        clip_id: ClipId,
        new_duration_beats: f64,
    },
    HalveNoteClip(TrackId, ClipId),
    ToggleEditMode,
    QuantizeNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
}

impl PianoRollMsg {
    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            PianoRollMsg::SelectNoteClip(..)
                | PianoRollMsg::SelectNote(..)
                | PianoRollMsg::SelectAllNotes(..)
                | PianoRollMsg::SelectNotesInRegion { .. }
                | PianoRollMsg::ScrollY(_)
                | PianoRollMsg::ToggleEditMode
        )
    }
}

/// Read-only cross-domain facts for piano roll updates.
#[derive(Debug, Clone, Copy)]
pub struct PianoRollCtx {
    /// Current snap grid (quantize target).
    pub snap_grid: SnapGrid,
}

impl Default for PianoRollCtx {
    fn default() -> Self {
        Self {
            snap_grid: SnapGrid::EIGHTH,
        }
    }
}

/// Cross-domain effects requested by a piano roll update.
#[derive(Debug, Default, PartialEq)]
pub struct PianoRollAction {
    /// Status bar text.
    pub status: Option<String>,
    /// Select this note clip for editing (arrangement owns selection).
    pub select_note_clip: Option<(TrackId, ClipId)>,
    /// Also select the owning track.
    pub select_track: Option<TrackId>,
    /// A resize moved a clip edge near the view boundary; auto-scroll.
    pub scroll_to_beat: Option<f64>,
    /// A resize drag is in flight (suppresses click-through selection).
    pub drag_resize_active: bool,
}

fn find_track(timeline: &TimelineContent, track_id: TrackId) -> Option<&TrackTimelineContent> {
    timeline.get(track_id)
}

fn find_track_mut(
    timeline: &mut TimelineContent,
    track_id: TrackId,
) -> Option<&mut TrackTimelineContent> {
    timeline.get_mut(track_id)
}

fn find_note_clip_mut(
    timeline: &mut TimelineContent,
    track_id: TrackId,
    clip_id: ClipId,
) -> Option<&mut UiNoteClip> {
    find_track_mut(timeline, track_id)?
        .note_clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
}

/// Snap every note start to the grid and sync changed notes to the
/// engine.
fn quantize_note_clip(
    tracks: &mut TimelineContent,
    track_id: TrackId,
    clip_id: ClipId,
    grid: SnapGrid,
    engine: &mut impl EngineHandle,
    action: &mut PianoRollAction,
) {
    let mut changes: Vec<(usize, MidiNote)> = Vec::new();
    let Some(track) = find_track_mut(tracks, track_id) else {
        return;
    };
    let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) else {
        return;
    };
    for (idx, note) in clip.notes.iter_mut().enumerate() {
        let snapped = grid.snap_beat(note.start_beat).max(0.0);
        if (snapped - note.start_beat).abs() > f64::EPSILON {
            note.start_beat = snapped;
            changes.push((idx, *note));
        }
    }
    let count = changes.len();
    for (idx, note) in changes {
        engine.send(EngineCommand::EditNote {
            track_id,
            clip_id,
            note_index: idx,
            note,
        });
    }
    action.status = Some(format!("Quantized {count} note(s) to {}", grid.label()));
}

impl PianoRollState {
    pub fn update(
        &mut self,
        msg: PianoRollMsg,
        engine: &mut impl EngineHandle,
        editor: &mut TimelineEditorState,
        ctx: PianoRollCtx,
    ) -> PianoRollAction {
        let tracks = Arc::make_mut(&mut editor.timeline);
        let mut action = PianoRollAction::default();
        match msg {
            PianoRollMsg::ToggleNoteClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        // Activating Loop starts from the complete Clip. A
                        // shorter musical loop is an explicit edit made with
                        // the piano-roll markers, never inferred from notes or
                        // retained from a stale pre-resize region.
                        if clip.loop_enabled {
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        cmd_data = Some((
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }
                if let Some((enabled, loop_start_beats, loop_end_beats)) = cmd_data {
                    engine.send(EngineCommand::SetNoteClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                }
            }
            PianoRollMsg::SetNoteClipGrooveGrid(track_id, clip_id, groove_grid) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|clip| clip.id == clip_id)
                    {
                        clip.groove_grid = groove_grid;
                    }
                }
                engine.send(EngineCommand::SetNoteClipGrooveGrid {
                    track_id,
                    clip_id,
                    groove_grid,
                });
                action.status = Some(match groove_grid {
                    GrooveGrid::Off => "Clip Swing disabled".to_string(),
                    _ => format!("Clip Swing follows Track on {}", groove_grid.label()),
                });
            }
            PianoRollMsg::SetNoteClipLoopRegion {
                track_id,
                clip_id,
                loop_start_beats,
                loop_end_beats,
            } => {
                let mut command = None;
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let valid = loop_start_beats.is_finite()
                            && loop_end_beats.is_finite()
                            && loop_start_beats >= 0.0
                            && loop_start_beats < loop_end_beats
                            && loop_end_beats <= clip.duration_beats;
                        if valid {
                            clip.loop_start_beats = loop_start_beats;
                            clip.loop_end_beats = loop_end_beats;
                            command = Some(clip.loop_enabled);
                        }
                    }
                }
                if let Some(enabled) = command {
                    engine.send(EngineCommand::SetNoteClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                }
            }
            PianoRollMsg::AddNoteClipToTrack(track_id) => {
                let clip_id = ClipId::new();
                let position_beats = 0.0;
                let duration_beats = 16.0;
                let track = tracks.ensure(track_id);
                track.note_clips.push(UiNoteClip {
                    id: clip_id,
                    name: format!("Pattern {}", track.note_clips.len() + 1),
                    position_beats,
                    duration_beats,
                    notes: Vec::new(),
                    selected_notes: HashSet::new(),
                    loop_enabled: true,
                    loop_start_beats: 0.0,
                    loop_end_beats: duration_beats,
                    groove_grid: vibez_core::perform::GrooveGrid::Off,
                });
                engine.send(EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled: true,
                    loop_start_beats: 0.0,
                    loop_end_beats: duration_beats,
                    groove_grid: vibez_core::perform::GrooveGrid::Off,
                });
                // Auto-select the new note clip for piano roll editing
                action.select_note_clip = Some((track_id, clip_id));
                action.status = Some("Added note clip".to_string());
            }
            PianoRollMsg::SelectNoteClip(track_id, clip_id) => {
                action.select_note_clip = Some((track_id, clip_id));
                action.select_track = Some(track_id);
            }
            PianoRollMsg::AddNote {
                track_id,
                clip_id,
                pitch,
                start_beat,
                duration_beats,
            } => {
                let note = MidiNote {
                    pitch,
                    velocity: 100,
                    start_beat,
                    duration_beats,
                };
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.notes.push(note);
                    }
                }
                engine.send(EngineCommand::AddNote {
                    track_id,
                    clip_id,
                    note,
                });
            }
            PianoRollMsg::RemoveNote(track_id, clip_id, note_index) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes.remove(note_index);
                            // Re-index: remove deleted index, shift down any higher indices
                            clip.selected_notes.remove(&note_index);
                            clip.selected_notes = clip
                                .selected_notes
                                .iter()
                                .map(|&i| if i > note_index { i - 1 } else { i })
                                .collect();
                        }
                    }
                }
                engine.send(EngineCommand::RemoveNote {
                    track_id,
                    clip_id,
                    note_index,
                });
            }
            PianoRollMsg::EditNote(track_id, clip_id, note_index, new_note) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes[note_index] = new_note;
                        }
                    }
                }
                engine.send(EngineCommand::EditNote {
                    track_id,
                    clip_id,
                    note_index,
                    note: new_note,
                });
            }
            PianoRollMsg::SetNoteVelocities {
                track_id,
                clip_id,
                velocities,
            } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(clip) = find_note_clip_mut(tracks, track_id, clip_id) {
                    for (note_index, velocity) in velocities {
                        let Some(note) = clip.notes.get_mut(note_index) else {
                            continue;
                        };
                        note.velocity = velocity.clamp(1, 127);
                        updates.push((note_index, *note));
                    }
                }
                for (note_index, note) in updates {
                    engine.send(EngineCommand::EditNote {
                        track_id,
                        clip_id,
                        note_index,
                        note,
                    });
                }
            }
            PianoRollMsg::SelectNote(track_id, clip_id, note_index, shift_held) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        match note_index {
                            Some(idx) => {
                                if shift_held {
                                    // Toggle note in/out of selection
                                    if !clip.selected_notes.remove(&idx) {
                                        clip.selected_notes.insert(idx);
                                    }
                                } else {
                                    // Clear all, select only this note
                                    clip.selected_notes.clear();
                                    clip.selected_notes.insert(idx);
                                }
                            }
                            None => {
                                clip.selected_notes.clear();
                            }
                        }
                    }
                }
            }
            PianoRollMsg::SelectAllNotes(track_id, clip_id) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.selected_notes = (0..clip.notes.len()).collect();
                    }
                }
            }
            PianoRollMsg::SelectNotesInRegion {
                track_id,
                clip_id,
                start_beat,
                end_beat,
                low_pitch,
                high_pitch,
                additive,
            } => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if !additive {
                            clip.selected_notes.clear();
                        }
                        for (index, note) in clip.notes.iter().enumerate() {
                            // Overlap, not containment: a note only clipped
                            // by the edge of the box still reads as caught.
                            let overlaps_time = note.start_beat < end_beat
                                && note.start_beat + note.duration_beats > start_beat;
                            if overlaps_time && (low_pitch..=high_pitch).contains(&note.pitch) {
                                clip.selected_notes.insert(index);
                            }
                        }
                    }
                }
            }
            PianoRollMsg::RemoveSelectedNotes(track_id, clip_id) => {
                // Collect indices to remove in reverse order
                let indices_to_remove: Vec<usize> =
                    if let Some(track) = find_track(tracks, track_id) {
                        if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                            let mut indices: Vec<usize> = clip
                                .selected_notes
                                .iter()
                                .copied()
                                .filter(|&i| i < clip.notes.len())
                                .collect();
                            indices.sort_unstable_by(|a, b| b.cmp(a));
                            indices
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };

                // Remove from engine in reverse order (indices stay valid)
                for &idx in &indices_to_remove {
                    engine.send(EngineCommand::RemoveNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                    });
                }

                // Remove from UI state
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        for &idx in &indices_to_remove {
                            if idx < clip.notes.len() {
                                clip.notes.remove(idx);
                            }
                        }
                        clip.selected_notes.clear();
                    }
                }
            }
            PianoRollMsg::NudgeSelectedNotes {
                track_id,
                clip_id,
                delta_beats,
                delta_semitones,
            } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let indices: Vec<usize> = clip
                            .selected_notes
                            .iter()
                            .copied()
                            .filter(|&i| i < clip.notes.len())
                            .collect();
                        for &idx in &indices {
                            let note = &mut clip.notes[idx];
                            note.start_beat = (note.start_beat + delta_beats).max(0.0);
                            note.pitch =
                                (note.pitch as i16 + delta_semitones as i16).clamp(0, 127) as u8;
                            updates.push((idx, *note));
                        }
                    }
                }
                for (idx, note) in updates {
                    engine.send(EngineCommand::EditNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                        note,
                    });
                }
            }
            PianoRollMsg::MoveNotesAbsolute {
                track_id,
                clip_id,
                moves,
            } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        for &(idx, new_beat, new_pitch) in &moves {
                            if idx < clip.notes.len() {
                                clip.notes[idx].start_beat = new_beat;
                                clip.notes[idx].pitch = new_pitch;
                                updates.push((idx, clip.notes[idx]));
                            }
                        }
                    }
                }
                for (idx, note) in updates {
                    engine.send(EngineCommand::EditNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                        note,
                    });
                }
            }
            PianoRollMsg::DoubleNoteClip(track_id, clip_id) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let orig_dur = clip.duration_beats;
                        let cloned_notes: Vec<MidiNote> = clip
                            .notes
                            .iter()
                            .map(|n| MidiNote {
                                start_beat: n.start_beat + orig_dur,
                                ..*n
                            })
                            .collect();
                        clip.notes.extend_from_slice(&cloned_notes);
                        let was_full_clip_loop = clip.loop_enabled
                            && clip.loop_start_beats == 0.0
                            && (clip.loop_end_beats - clip.duration_beats).abs() < 1e-9;
                        clip.duration_beats *= 2.0;
                        if was_full_clip_loop {
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        let new_duration = clip.duration_beats;
                        let loop_sync = (
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        );

                        // Send new notes to engine
                        for note in &cloned_notes {
                            engine.send(EngineCommand::AddNote {
                                track_id,
                                clip_id,
                                note: *note,
                            });
                        }
                        // The engine clip must grow too, or playback
                        // still ends at the old boundary and the
                        // duplicated notes never sound.
                        engine.send(EngineCommand::SetNoteClipDuration {
                            track_id,
                            clip_id,
                            duration_beats: new_duration,
                        });
                        engine.send(EngineCommand::SetNoteClipLoop {
                            track_id,
                            clip_id,
                            enabled: loop_sync.0,
                            loop_start_beats: loop_sync.1,
                            loop_end_beats: loop_sync.2,
                        });
                    }
                }
                action.status = Some("Doubled clip length".to_string());
            }
            PianoRollMsg::CropNoteClip(track_id, clip_id) => {
                let mut sync_data = None;
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if !clip.notes.is_empty() {
                            let min_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat)
                                .fold(f64::INFINITY, f64::min);
                            let max_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat + n.duration_beats)
                                .fold(f64::NEG_INFINITY, f64::max);

                            // Shift notes so first note starts at 0
                            for note in &mut clip.notes {
                                note.start_beat -= min_beat;
                            }
                            clip.position_beats += min_beat;
                            clip.duration_beats = max_beat - min_beat;

                            sync_data = Some((
                                clip.position_beats,
                                clip.duration_beats,
                                clip.notes.clone(),
                                clip.groove_grid,
                            ));
                        }
                    }
                }
                // Sync to engine outside the mutable borrow
                if let Some((pos, dur, notes, groove_grid)) = sync_data {
                    engine.send(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                        groove_grid,
                    });
                    for note in &notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
                action.status = Some("Cropped clip to content".to_string());
            }
            PianoRollMsg::ScrollY(y) => {
                self.scroll_y = y;
            }
            PianoRollMsg::ResizeNoteClipDuration {
                track_id,
                clip_id,
                new_duration_beats,
            } => {
                let mut sync_data = None;
                let mut clip_end_beat = None;
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.duration_beats = new_duration_beats;

                        // Keep the loop region inside the clip when
                        // shrinking. Extending leaves the region
                        // untouched so the looped pattern repeats to
                        // fill the new length (the whole point of
                        // stretching a looped clip).
                        if clip.loop_enabled && clip.loop_end_beats > new_duration_beats {
                            clip.loop_end_beats = new_duration_beats;
                            if clip.loop_start_beats >= clip.loop_end_beats {
                                clip.loop_start_beats = 0.0;
                            }
                        }

                        clip_end_beat = Some(clip.position_beats + clip.duration_beats);
                        sync_data = Some((
                            clip.position_beats,
                            clip.duration_beats,
                            clip.notes.clone(),
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                            clip.groove_grid,
                        ));
                    }
                }
                // Sync to engine via Remove+Add+re-add-notes (loop state included atomically)
                if let Some((
                    pos,
                    dur,
                    notes,
                    loop_enabled,
                    loop_start_beats,
                    loop_end_beats,
                    groove_grid,
                )) = sync_data
                {
                    engine.send(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled,
                        loop_start_beats,
                        loop_end_beats,
                        groove_grid,
                    });
                    for note in &notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
                if let Some(end_beat) = clip_end_beat {
                    action.scroll_to_beat = Some(end_beat);
                }
                action.drag_resize_active = true;
            }
            PianoRollMsg::HalveNoteClip(track_id, clip_id) => {
                if let Some(track) = find_track_mut(tracks, track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let new_dur = (clip.duration_beats / 2.0).max(0.25);
                        clip.duration_beats = new_dur;
                        engine.send(EngineCommand::SetNoteClipDuration {
                            track_id,
                            clip_id,
                            duration_beats: new_dur,
                        });
                    }
                }
                action.status = Some("Halved clip duration".to_string());
            }
            PianoRollMsg::ToggleEditMode => {
                use crate::state::PianoRollEditMode;
                self.edit_mode = match self.edit_mode {
                    PianoRollEditMode::Select => PianoRollEditMode::Draw,
                    PianoRollEditMode::Draw => PianoRollEditMode::Select,
                };
                let mode_name = match self.edit_mode {
                    PianoRollEditMode::Select => "Select",
                    PianoRollEditMode::Draw => "Draw",
                };
                action.status = Some(format!("Piano roll: {mode_name} mode"));
            }
            PianoRollMsg::QuantizeNoteClip { track_id, clip_id } => {
                quantize_note_clip(
                    tracks,
                    track_id,
                    clip_id,
                    ctx.snap_grid,
                    engine,
                    &mut action,
                );
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;
    use crate::state::PianoRollEditMode;

    fn midi_track_with_clip() -> (TimelineEditorState, TrackId, ClipId) {
        let track_id = TrackId::new();
        let clip_id = ClipId::new();
        let mut timeline = TimelineEditorState::default();
        let track = timeline.ensure(track_id);
        track.note_clips.push(UiNoteClip {
            id: clip_id,
            name: "Pattern 1".to_string(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: vec![
                MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.1,
                    duration_beats: 0.5,
                },
                MidiNote {
                    pitch: 64,
                    velocity: 100,
                    start_beat: 1.9,
                    duration_beats: 0.5,
                },
            ],
            selected_notes: HashSet::new(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        });
        (timeline, track_id, clip_id)
    }

    /// The fixture holds two notes: pitch 60 at beats 0.1..0.6, and
    /// pitch 64 at beats 1.9..2.4.
    #[allow(clippy::too_many_arguments)]
    fn region(
        tracks: &mut TimelineEditorState,
        tid: TrackId,
        cid: ClipId,
        start_beat: f64,
        end_beat: f64,
        low_pitch: u8,
        high_pitch: u8,
        additive: bool,
    ) {
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        pr.update(
            PianoRollMsg::SelectNotesInRegion {
                track_id: tid,
                clip_id: cid,
                start_beat,
                end_beat,
                low_pitch,
                high_pitch,
                additive,
            },
            &mut engine,
            tracks,
            PianoRollCtx::default(),
        );
    }

    fn selected(tracks: &TimelineEditorState, tid: TrackId) -> HashSet<usize> {
        tracks.get(tid).unwrap().note_clips[0]
            .selected_notes
            .clone()
    }

    #[test]
    fn a_rubber_band_catches_notes_inside_both_its_beat_and_pitch_range() {
        let (mut tracks, tid, cid) = midi_track_with_clip();

        region(&mut tracks, tid, cid, 0.0, 4.0, 0, 127, false);
        assert_eq!(selected(&tracks, tid), HashSet::from([0, 1]));

        // Same beats, pitch range excluding the second note.
        region(&mut tracks, tid, cid, 0.0, 4.0, 55, 62, false);
        assert_eq!(selected(&tracks, tid), HashSet::from([0]));

        // Same pitches, beat range excluding the first note.
        region(&mut tracks, tid, cid, 1.0, 4.0, 0, 127, false);
        assert_eq!(selected(&tracks, tid), HashSet::from([1]));
    }

    #[test]
    fn a_note_only_clipped_by_the_edge_of_the_band_still_counts() {
        let (mut tracks, tid, cid) = midi_track_with_clip();

        // Ends at 0.3, partway through the first note's 0.1..0.6 span.
        region(&mut tracks, tid, cid, 0.0, 0.3, 0, 127, false);

        assert_eq!(selected(&tracks, tid), HashSet::from([0]));
    }

    #[test]
    fn a_non_additive_band_replaces_the_selection_while_shift_extends_it() {
        let (mut tracks, tid, cid) = midi_track_with_clip();

        region(&mut tracks, tid, cid, 0.0, 1.0, 0, 127, false);
        assert_eq!(selected(&tracks, tid), HashSet::from([0]));

        // Non-additive band elsewhere drops the earlier note.
        region(&mut tracks, tid, cid, 1.5, 4.0, 0, 127, false);
        assert_eq!(selected(&tracks, tid), HashSet::from([1]));

        // Additive band brings it back alongside.
        region(&mut tracks, tid, cid, 0.0, 1.0, 0, 127, true);
        assert_eq!(selected(&tracks, tid), HashSet::from([0, 1]));
    }

    #[test]
    fn add_note_updates_clip_and_engine() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        pr.update(
            PianoRollMsg::AddNote {
                track_id: tid,
                clip_id: cid,
                pitch: 67,
                start_beat: 2.0,
                duration_beats: 1.0,
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        assert_eq!(tracks.get(tid).unwrap().note_clips[0].notes.len(), 3);
        assert!(matches!(engine.0[0], EngineCommand::AddNote { .. }));
    }

    #[test]
    fn batch_velocity_edit_updates_valid_notes_and_the_engine() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        piano_roll.update(
            PianoRollMsg::SetNoteVelocities {
                track_id: tid,
                clip_id: cid,
                velocities: vec![(0, 72), (1, 118), (99, 64)],
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );

        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.notes[0].velocity, 72);
        assert_eq!(clip.notes[1].velocity, 118);
        assert_eq!(engine.0.len(), 2);
        assert!(matches!(
            engine.0[0],
            EngineCommand::EditNote {
                note_index: 0,
                note: MidiNote { velocity: 72, .. },
                ..
            }
        ));
        assert!(matches!(
            engine.0[1],
            EngineCommand::EditNote {
                note_index: 1,
                note: MidiNote { velocity: 118, .. },
                ..
            }
        ));
    }

    #[test]
    fn velocity_edit_clamps_zero_without_changing_shared_selection() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        tracks.get_mut(tid).unwrap().note_clips[0].selected_notes = HashSet::from([0, 1]);
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        piano_roll.update(
            PianoRollMsg::SetNoteVelocities {
                track_id: tid,
                clip_id: cid,
                velocities: vec![(0, 0), (1, u8::MAX)],
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );

        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.notes[0].velocity, 1);
        assert_eq!(clip.notes[1].velocity, 127);
        assert_eq!(clip.selected_notes, HashSet::from([0, 1]));
    }

    #[test]
    fn add_note_clip_creates_missing_timeline_lane_for_a_shared_track() {
        let track_id = TrackId::new();
        let mut editor = TimelineEditorState::default();
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        let action = piano_roll.update(
            PianoRollMsg::AddNoteClipToTrack(track_id),
            &mut engine,
            &mut editor,
            PianoRollCtx::default(),
        );

        assert_eq!(editor.timeline.get(track_id).unwrap().note_clips.len(), 1);
        assert_eq!(action.select_note_clip.unwrap().0, track_id);
        assert!(matches!(engine.0[0], EngineCommand::AddNoteClip { .. }));
    }

    #[test]
    fn remove_note_reindexes_selection() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        tracks.get_mut(tid).unwrap().note_clips[0].selected_notes = [0, 1].into_iter().collect();
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        pr.update(
            PianoRollMsg::RemoveNote(tid, cid, 0),
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.notes.len(), 1);
        assert_eq!(
            clip.selected_notes.iter().copied().collect::<Vec<_>>(),
            vec![0]
        );
    }

    #[test]
    fn quantize_snaps_notes_and_reports_count() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        let action = pr.update(
            PianoRollMsg::QuantizeNoteClip {
                track_id: tid,
                clip_id: cid,
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx {
                snap_grid: SnapGrid::QUARTER,
            },
        );
        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.notes[0].start_beat, 0.0);
        assert_eq!(clip.notes[1].start_beat, 2.0);
        assert_eq!(action.status.as_deref(), Some("Quantized 2 note(s) to 1/4"));
    }

    #[test]
    fn double_clip_clones_notes_and_grows_engine_clip() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        pr.update(
            PianoRollMsg::DoubleNoteClip(tid, cid),
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.duration_beats, 8.0);
        assert_eq!(clip.notes.len(), 4);
        assert_eq!(clip.notes[2].start_beat, 4.1);
        assert!(engine
            .0
            .iter()
            .any(|c| matches!(c, EngineCommand::SetNoteClipDuration { .. })));
    }

    #[test]
    fn toggle_loop_defaults_region_to_the_complete_clip() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        pr.update(
            PianoRollMsg::ToggleNoteClipLoop(tid, cid),
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert!(clip.loop_enabled);
        assert_eq!(clip.loop_start_beats, 0.0);
        assert_eq!(clip.loop_end_beats, 4.0);
        assert!(matches!(engine.0[0], EngineCommand::SetNoteClipLoop { .. }));
    }

    #[test]
    fn enabling_loop_replaces_stale_bounds_with_the_current_clip_length() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let clip = &mut tracks.get_mut(tid).unwrap().note_clips[0];
        clip.duration_beats = 8.0;
        clip.loop_enabled = false;
        clip.loop_start_beats = 0.0;
        clip.loop_end_beats = 4.0;
        clip.notes.push(MidiNote {
            pitch: 67,
            velocity: 100,
            start_beat: 6.0,
            duration_beats: 0.5,
        });

        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        piano_roll.update(
            PianoRollMsg::ToggleNoteClipLoop(tid, cid),
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );

        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert!(clip.loop_enabled);
        assert_eq!(clip.loop_start_beats, 0.0);
        assert_eq!(clip.loop_end_beats, 8.0);
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::SetNoteClipLoop {
                enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 8.0,
                ..
            }]
        ));
    }

    #[test]
    fn extending_a_non_looping_clip_does_not_infer_a_loop_from_note_content() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        piano_roll.update(
            PianoRollMsg::ResizeNoteClipDuration {
                track_id: tid,
                clip_id: cid,
                new_duration_beats: 8.0,
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );

        let clip = &tracks.get(tid).unwrap().note_clips[0];
        assert_eq!(clip.duration_beats, 8.0);
        assert!(!clip.loop_enabled);
        assert_eq!(clip.loop_start_beats, 0.0);
        assert_eq!(clip.loop_end_beats, 0.0);
    }

    #[test]
    fn midi_loop_region_must_be_ordered_and_inside_the_clip() {
        let (mut tracks, track_id, clip_id) = midi_track_with_clip();
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        piano_roll.update(
            PianoRollMsg::SetNoteClipLoopRegion {
                track_id,
                clip_id,
                loop_start_beats: 3.0,
                loop_end_beats: 5.0,
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        let clip = &tracks.get(track_id).unwrap().note_clips[0];
        assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (0.0, 0.0));
        assert!(engine.0.is_empty());

        piano_roll.update(
            PianoRollMsg::SetNoteClipLoopRegion {
                track_id,
                clip_id,
                loop_start_beats: 1.0,
                loop_end_beats: 3.0,
            },
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        let clip = &tracks.get(track_id).unwrap().note_clips[0];
        assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (1.0, 3.0));
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::SetNoteClipLoop { .. }]
        ));
    }

    #[test]
    fn groove_grid_updates_the_clip_and_live_engine_source() {
        let (mut tracks, tid, cid) = midi_track_with_clip();
        let mut piano_roll = PianoRollState::default();
        let mut engine = RecordingEngine::default();

        let action = piano_roll.update(
            PianoRollMsg::SetNoteClipGrooveGrid(tid, cid, GrooveGrid::Sixteenth),
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );

        assert_eq!(
            tracks.get(tid).unwrap().note_clips[0].groove_grid,
            GrooveGrid::Sixteenth
        );
        assert!(matches!(
            engine.0[0],
            EngineCommand::SetNoteClipGrooveGrid {
                track_id,
                clip_id,
                groove_grid: GrooveGrid::Sixteenth,
            } if track_id == tid && clip_id == cid
        ));
        assert_eq!(
            action.status.as_deref(),
            Some("Clip Swing follows Track on 1/16")
        );
    }

    #[test]
    fn toggle_edit_mode_flips_and_reports() {
        let mut pr = PianoRollState::default();
        let mut engine = RecordingEngine::default();
        let mut tracks = TimelineEditorState::default();
        let action = pr.update(
            PianoRollMsg::ToggleEditMode,
            &mut engine,
            &mut tracks,
            PianoRollCtx::default(),
        );
        assert_eq!(pr.edit_mode, PianoRollEditMode::Draw);
        assert_eq!(action.status.as_deref(), Some("Piano roll: Draw mode"));
    }
}
