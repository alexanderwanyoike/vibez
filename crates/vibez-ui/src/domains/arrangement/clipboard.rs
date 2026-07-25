//! One clip clipboard shared by Arrange and every Section timeline.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_engine::commands::EngineCommand;

use super::{ArrangementAction, ArrangementCtx, ArrangementMsg, EngineHandle};
use crate::state::{
    ArrangementSelection, ClipClipboard, ClipboardClip, ProjectTracksState, TimelineEditorState,
};

impl ClipboardClip {
    pub(super) fn source_track(&self) -> TrackId {
        match self {
            Self::Audio { track_id, .. } | Self::Note { track_id, .. } => *track_id,
        }
    }

    fn set_track_offset(&mut self, offset: usize) {
        match self {
            Self::Audio { track_offset, .. } | Self::Note { track_offset, .. } => {
                *track_offset = offset;
            }
        }
    }

    fn track_offset(&self) -> usize {
        match self {
            Self::Audio { track_offset, .. } | Self::Note { track_offset, .. } => *track_offset,
        }
    }
}

impl TimelineEditorState {
    pub(crate) fn update_clipboard(
        &mut self,
        project_tracks: &ProjectTracksState,
        msg: ArrangementMsg,
        clipboard: &mut ClipClipboard,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        match msg {
            ArrangementMsg::CopySelectedClips => {
                self.op_copy_selected_clips(project_tracks, clipboard, ctx)
            }
            ArrangementMsg::CutSelectedClips => {
                let start = self.selection_start_beats;
                let end = self.selection_end_beats;
                let track_id = self.time_selection_track;
                let ranged = self.time_selection_active && end > start;
                let copy = self.op_copy_selected_clips(project_tracks, clipboard, ctx);
                if copy.status.as_deref() == Some("Nothing to copy") {
                    return copy;
                }

                let mut action = if ranged {
                    self.op_delete_clips_in_region(engine, ctx, start, end, track_id)
                } else {
                    self.remove_selected_clips(engine)
                };
                action.status = Some("Cut to clipboard".to_string());
                action.mark_dirty = true;
                action
            }
            ArrangementMsg::PasteClips => {
                self.op_paste_clips(project_tracks, clipboard, engine, ctx)
            }
            _ => unreachable!("only clipboard messages enter the clipboard boundary"),
        }
    }

    fn remove_selected_clips(&mut self, engine: &mut impl EngineHandle) -> ArrangementAction {
        let selections: Vec<_> = self.selected_clips.drain().collect();
        for selection in &selections {
            match selection {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    engine.send(EngineCommand::RemoveClip(*track_id, *clip_id));
                    if let Some(track) = self.find_content_mut(*track_id) {
                        track.clips.retain(|clip| clip.id != *clip_id);
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } => {
                    engine.send(EngineCommand::RemoveNoteClip(*track_id, *clip_id));
                    if let Some(track) = self.find_content_mut(*track_id) {
                        track.note_clips.retain(|clip| clip.id != *clip_id);
                    }
                    if self
                        .selected_note_clip
                        .is_some_and(|selected| selected == (*track_id, *clip_id))
                    {
                        self.selected_note_clip = None;
                    }
                }
            }
        }
        ArrangementAction::default()
    }

    fn op_copy_selected_clips(
        &self,
        project_tracks: &ProjectTracksState,
        clipboard: &mut ClipClipboard,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut copied = Vec::new();
        let spb = ctx.samples_per_beat;

        if self.time_selection_active
            && self.selection_end_beats > self.selection_start_beats
            && spb > 0.0
        {
            let start = self.selection_start_beats;
            let end = self.selection_end_beats;
            for (track_id, content) in self.timeline.by_track.iter().filter(|(track_id, _)| {
                self.time_selection_track
                    .is_none_or(|target| **track_id == target)
            }) {
                for clip in &content.clips {
                    let clip_start = clip.position as f64 / spb;
                    let clip_end = (clip.position + clip.duration) as f64 / spb;
                    let overlap_start = clip_start.max(start);
                    let overlap_end = clip_end.min(end);
                    if overlap_end <= overlap_start {
                        continue;
                    }
                    let delta = ((overlap_start - clip_start) * spb).round() as u64;
                    let duration = ((overlap_end - overlap_start) * spb).round() as u64;
                    let mut fragment = clip.clone();
                    fragment.position = 0;
                    fragment.duration = duration.max(1);
                    let raw_offset = clip.source_offset.saturating_add(delta);
                    fragment.source_offset = if clip.loop_enabled && clip.loop_end > clip.loop_start
                    {
                        if raw_offset >= clip.loop_end {
                            clip.loop_start
                                + (raw_offset - clip.loop_start) % (clip.loop_end - clip.loop_start)
                        } else {
                            raw_offset
                        }
                    } else {
                        raw_offset
                    };
                    copied.push(ClipboardClip::Audio {
                        track_id: *track_id,
                        track_offset: 0,
                        position_beats: overlap_start,
                        clip: fragment,
                    });
                }

                for clip in &content.note_clips {
                    let clip_end = clip.position_beats + clip.duration_beats;
                    let overlap_start = clip.position_beats.max(start);
                    let overlap_end = clip_end.min(end);
                    if overlap_end <= overlap_start {
                        continue;
                    }
                    let local_start = overlap_start - clip.position_beats;
                    let local_end = overlap_end - clip.position_beats;
                    let mut notes = Vec::new();
                    let looping = clip.loop_enabled && clip.loop_end_beats > clip.loop_start_beats;
                    for note in &clip.notes {
                        let mut occurrences = vec![note.start_beat];
                        if looping
                            && note.start_beat >= clip.loop_start_beats
                            && note.start_beat < clip.loop_end_beats
                        {
                            let loop_len = clip.loop_end_beats - clip.loop_start_beats;
                            let mut occurrence = note.start_beat + loop_len;
                            while occurrence < local_end {
                                occurrences.push(occurrence);
                                occurrence += loop_len;
                            }
                        }
                        for occurrence in occurrences {
                            let kept_start = occurrence.max(local_start);
                            let kept_end = (occurrence + note.duration_beats).min(local_end);
                            if kept_end > kept_start {
                                notes.push(MidiNote {
                                    start_beat: kept_start - local_start,
                                    duration_beats: kept_end - kept_start,
                                    ..*note
                                });
                            }
                        }
                    }
                    let mut fragment = clip.clone();
                    fragment.position_beats = 0.0;
                    fragment.duration_beats = overlap_end - overlap_start;
                    fragment.notes = notes;
                    fragment.selected_notes.clear();
                    fragment.loop_enabled = false;
                    fragment.loop_start_beats = 0.0;
                    fragment.loop_end_beats = 0.0;
                    copied.push(ClipboardClip::Note {
                        track_id: *track_id,
                        track_offset: 0,
                        position_beats: overlap_start,
                        clip: fragment,
                    });
                }
            }
        } else {
            for selection in &self.selected_clips {
                match selection {
                    ArrangementSelection::AudioClip { track_id, clip_id } if spb > 0.0 => {
                        if let Some(clip) = self.find_content(*track_id).and_then(|content| {
                            content.clips.iter().find(|clip| clip.id == *clip_id)
                        }) {
                            copied.push(ClipboardClip::Audio {
                                track_id: *track_id,
                                track_offset: 0,
                                position_beats: clip.position as f64 / spb,
                                clip: clip.clone(),
                            });
                        }
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        if let Some(clip) = self.find_content(*track_id).and_then(|content| {
                            content.note_clips.iter().find(|clip| clip.id == *clip_id)
                        }) {
                            copied.push(ClipboardClip::Note {
                                track_id: *track_id,
                                track_offset: 0,
                                position_beats: clip.position_beats,
                                clip: clip.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let top_index = copied
            .iter()
            .filter_map(|entry| {
                project_tracks
                    .tracks
                    .iter()
                    .position(|track| track.id == entry.source_track())
            })
            .min();
        if let Some(top_index) = top_index {
            for entry in &mut copied {
                if let Some(index) = project_tracks
                    .tracks
                    .iter()
                    .position(|track| track.id == entry.source_track())
                {
                    entry.set_track_offset(index - top_index);
                }
            }
        }

        let count = copied.len();
        if count > 0 {
            *clipboard = ClipClipboard { clips: copied };
        }
        ArrangementAction {
            status: Some(match count {
                0 => "Nothing to copy".to_string(),
                1 => "Copied clip".to_string(),
                _ => format!("Copied {count} clips"),
            }),
            ..Default::default()
        }
    }

    fn op_paste_clips(
        &mut self,
        project_tracks: &ProjectTracksState,
        clipboard: &ClipClipboard,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        if clipboard.clips.is_empty() {
            return ArrangementAction {
                status: Some("Clipboard is empty".to_string()),
                ..Default::default()
            };
        }
        if ctx.samples_per_beat <= 0.0 {
            return ArrangementAction {
                status: Some("Can't paste clips without a valid project tempo".to_string()),
                ..Default::default()
            };
        }
        let Some(anchor_track) = self.selected_track else {
            return ArrangementAction {
                status: Some("Select a destination Project Track".to_string()),
                ..Default::default()
            };
        };
        let Some(anchor_index) = project_tracks
            .tracks
            .iter()
            .position(|track| track.id == anchor_track)
        else {
            return ArrangementAction {
                status: Some("Select a destination Project Track".to_string()),
                ..Default::default()
            };
        };

        let mut destinations = Vec::with_capacity(clipboard.clips.len());
        for entry in &clipboard.clips {
            let Some(index) = anchor_index.checked_add(entry.track_offset()) else {
                return incompatible_paste("the Track layout runs past the project");
            };
            let Some(track) = project_tracks.tracks.get(index) else {
                return incompatible_paste("the Track layout runs past the project");
            };
            let compatible = match entry {
                ClipboardClip::Audio { .. } => !track.kind.is_midi(),
                ClipboardClip::Note { .. } => track.kind.is_midi(),
            };
            if !compatible {
                return incompatible_paste(&format!("{} has the wrong Clip type", track.name));
            }
            destinations.push(track.id);
        }

        let mut selected = HashSet::new();
        for (entry, track_id) in clipboard.clips.iter().cloned().zip(destinations) {
            match entry {
                ClipboardClip::Audio {
                    position_beats,
                    mut clip,
                    ..
                } => {
                    clip.id = ClipId::new();
                    clip.position = (position_beats * ctx.samples_per_beat).round().max(0.0) as u64;
                    engine.send(EngineCommand::AddClip {
                        track_id,
                        clip_id: clip.id,
                        audio: Arc::clone(&clip.audio),
                        position: clip.position,
                        source_offset: clip.source_offset,
                        duration: clip.duration,
                        loop_enabled: clip.loop_enabled,
                        loop_start: clip.loop_start,
                        loop_end: clip.loop_end,
                    });
                    selected.insert(ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: clip.id,
                    });
                    Arc::make_mut(&mut self.timeline)
                        .ensure(track_id)
                        .clips
                        .push(clip);
                }
                ClipboardClip::Note {
                    position_beats,
                    mut clip,
                    ..
                } => {
                    clip.id = ClipId::new();
                    clip.position_beats = position_beats;
                    clip.selected_notes.clear();
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: clip.id,
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
                        groove_grid: clip.groove_grid,
                    });
                    for note in &clip.notes {
                        engine.send(EngineCommand::AddNote {
                            track_id,
                            clip_id: clip.id,
                            note: *note,
                        });
                    }
                    selected.insert(ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: clip.id,
                    });
                    Arc::make_mut(&mut self.timeline)
                        .ensure(track_id)
                        .note_clips
                        .push(clip);
                }
            }
        }

        let count = selected.len();
        self.selected_clips = selected;
        self.selected_track = Some(anchor_track);
        self.time_selection_active = false;
        ArrangementAction {
            status: Some(if count == 1 {
                "Pasted clip".to_string()
            } else {
                format!("Pasted {count} clips")
            }),
            mark_dirty: true,
            ..Default::default()
        }
    }
}

fn incompatible_paste(reason: &str) -> ArrangementAction {
    ArrangementAction {
        status: Some(format!("Can't paste clips: {reason}")),
        ..Default::default()
    }
}
