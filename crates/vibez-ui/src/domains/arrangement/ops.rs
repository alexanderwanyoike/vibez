//! Multi-clip and range operations for Arrange Timeline Content.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_engine::commands::EngineCommand;

use super::{ArrangementAction, ArrangementCtx, EngineHandle};
use crate::state::{
    ArrangementSelection, ClipClipboard, ClipboardClip, ProjectTracksState, TimelineEditorState,
    UiNoteClip,
};

impl TimelineEditorState {
    pub(super) fn op_delete_clips_in_region(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let target_track = track_id;
        let spb = ctx.samples_per_beat;
        // Preserve material outside the range. Splitting first turns
        // the selected span into whole clips that can be removed.
        let _ = self.op_split_clips_at_region(engine, ctx, start_beats, end_beats, track_id);
        // Collect clip IDs to remove
        let mut audio_removals: Vec<(TrackId, ClipId)> = Vec::new();
        let mut note_removals: Vec<(TrackId, ClipId)> = Vec::new();
        for (content_track_id, track) in &self.timeline.by_track {
            if let Some(tid) = target_track {
                if *content_track_id != tid {
                    continue;
                }
            }
            if spb > 0.0 {
                for clip in &track.clips {
                    let clip_start = clip.position as f64 / spb;
                    let clip_end = (clip.position + clip.duration) as f64 / spb;
                    if clip_start >= start_beats - 1e-9 && clip_end <= end_beats + 1e-9 {
                        audio_removals.push((*content_track_id, clip.id));
                    }
                }
            }
            for nc in &track.note_clips {
                let clip_end = nc.position_beats + nc.duration_beats;
                if nc.position_beats >= start_beats - 1e-9 && clip_end <= end_beats + 1e-9 {
                    note_removals.push((*content_track_id, nc.id));
                }
            }
        }
        for (tid, cid) in &audio_removals {
            engine.send(EngineCommand::RemoveClip(*tid, *cid));
            if let Some(track) = self.find_content_mut(*tid) {
                track.clips.retain(|c| c.id != *cid);
            }
        }
        for (tid, cid) in &note_removals {
            engine.send(EngineCommand::RemoveNoteClip(*tid, *cid));
            if let Some(track) = self.find_content_mut(*tid) {
                track.note_clips.retain(|c| c.id != *cid);
            }
        }
        self.selected_clips.clear();
        self.selected_note_clip = None;
        self.time_selection_active = false;
        let count = audio_removals.len() + note_removals.len();
        action.status = Some(format!("Deleted {count} clips in region"));
        action
    }

    pub(super) fn op_split_clips_at_region(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let target_track = track_id;
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
                self.timeline
                    .by_track
                    .iter()
                    .filter(|(tid, _)| target_track.is_none_or(|target| **tid == target))
                    .flat_map(|(tid, content)| {
                        content.clips.iter().filter_map(|c| {
                            let cs = c.position as f64 / spb;
                            let ce = (c.position + c.duration) as f64 / spb;
                            if boundary_beats > cs && boundary_beats < ce {
                                Some((*tid, c.id))
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
                .timeline
                .by_track
                .iter()
                .filter(|(tid, _)| target_track.is_none_or(|target| **tid == target))
                .flat_map(|(tid, content)| {
                    content.note_clips.iter().filter_map(|c| {
                        let ce = c.position_beats + c.duration_beats;
                        if boundary_beats > c.position_beats && boundary_beats < ce {
                            Some((*tid, c.id))
                        } else {
                            None
                        }
                    })
                })
                .collect();

            for (tid, cid) in audio_hits {
                let _ = self.op_split_audio_clip(engine, ctx, tid, cid, boundary_sample);
                split_count += 1;
            }
            for (tid, cid) in note_hits {
                let _ = self.op_split_note_clip(engine, ctx, tid, cid, boundary_beats);
                split_count += 1;
            }
        }

        if split_count > 0 {
            action.status = Some(format!("Split {split_count} clips at region boundaries"));
        }
        action
    }

    pub(super) fn op_create_clip_from_selection(
        &mut self,
        project_tracks: &ProjectTracksState,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if let Some(tid) = self.selected_track {
            if let Some(track) = project_tracks.find(tid) {
                if track.kind.is_midi() {
                    return self.op_create_note_clip_from_selection(
                        project_tracks,
                        engine,
                        ctx,
                        tid,
                    );
                } else {
                    action.status = Some("Select a time region on a MIDI track".to_string());
                }
            }
        } else {
            action.status = Some("No track selected".to_string());
        }
        action
    }

    pub(super) fn op_create_note_clip_from_selection(
        &mut self,
        project_tracks: &ProjectTracksState,
        engine: &mut impl EngineHandle,
        _ctx: ArrangementCtx,
        track_id: TrackId,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if !self.time_selection_active || self.selection_end_beats <= self.selection_start_beats {
            action.status = Some("No time selection active".to_string());
            return action;
        }
        if let Some(track) = project_tracks.find(track_id) {
            if !track.kind.is_midi() {
                action.status = Some("Can only create note clips on MIDI tracks".to_string());
                return action;
            }
        }
        let clip_id = ClipId::new();
        let position_beats = self.selection_start_beats;
        let duration_beats = self.selection_end_beats - self.selection_start_beats;
        let track = Arc::make_mut(&mut self.timeline).ensure(track_id);
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
            groove_grid: vibez_core::perform::GrooveGrid::Off,
        });
        engine.send(EngineCommand::AddNoteClip {
            track_id,
            clip_id,
            position_beats,
            duration_beats,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: vibez_core::perform::GrooveGrid::Off,
        });
        self.selected_note_clip = Some((track_id, clip_id));
        self.selected_clips.clear();
        self.selected_clips
            .insert(ArrangementSelection::NoteClip { track_id, clip_id });
        action.status = Some("Created note clip from selection".to_string());
        action
    }

    pub(super) fn op_join_selected_clips(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let _ = ctx;
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
        action
    }

    pub(super) fn op_copy_selected_clips(&mut self, ctx: ArrangementCtx) -> ArrangementAction {
        let mut copied = Vec::new();
        let spb = ctx.samples_per_beat;

        if self.time_selection_active
            && self.selection_end_beats > self.selection_start_beats
            && spb > 0.0
        {
            let start = self.selection_start_beats;
            let end = self.selection_end_beats;
            for (content_track_id, track) in self.timeline.by_track.iter().filter(|(tid, _)| {
                self.time_selection_track
                    .is_none_or(|target| **tid == target)
            }) {
                for clip in &track.clips {
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
                        track_id: *content_track_id,
                        offset_beats: overlap_start - start,
                        clip: fragment,
                    });
                }

                for clip in &track.note_clips {
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
                            let note_end = occurrence + note.duration_beats;
                            let kept_start = occurrence.max(local_start);
                            let kept_end = note_end.min(local_end);
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
                        track_id: *content_track_id,
                        offset_beats: overlap_start - start,
                        clip: fragment,
                    });
                }
            }
        } else {
            let mut starts = Vec::new();
            for selection in &self.selected_clips {
                match selection {
                    ArrangementSelection::AudioClip { track_id, clip_id } if spb > 0.0 => {
                        if let Some(clip) = self
                            .find_content(*track_id)
                            .and_then(|t| t.clips.iter().find(|c| c.id == *clip_id))
                        {
                            starts.push(clip.position as f64 / spb);
                        }
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        if let Some(clip) = self
                            .find_content(*track_id)
                            .and_then(|t| t.note_clips.iter().find(|c| c.id == *clip_id))
                        {
                            starts.push(clip.position_beats);
                        }
                    }
                    _ => {}
                }
            }
            let anchor = starts.into_iter().reduce(f64::min).unwrap_or(0.0);
            for selection in &self.selected_clips {
                match selection {
                    ArrangementSelection::AudioClip { track_id, clip_id } if spb > 0.0 => {
                        if let Some(clip) = self
                            .find_content(*track_id)
                            .and_then(|t| t.clips.iter().find(|c| c.id == *clip_id))
                        {
                            copied.push(ClipboardClip::Audio {
                                track_id: *track_id,
                                offset_beats: clip.position as f64 / spb - anchor,
                                clip: clip.clone(),
                            });
                        }
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        if let Some(clip) = self
                            .find_content(*track_id)
                            .and_then(|t| t.note_clips.iter().find(|c| c.id == *clip_id))
                        {
                            copied.push(ClipboardClip::Note {
                                track_id: *track_id,
                                offset_beats: clip.position_beats - anchor,
                                clip: clip.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let count = copied.len();
        if count > 0 {
            self.clipboard = ClipClipboard { clips: copied };
        }
        ArrangementAction {
            status: Some(if count == 0 {
                "Nothing to copy".to_string()
            } else if count == 1 {
                "Copied clip".to_string()
            } else {
                format!("Copied {count} clips")
            }),
            ..Default::default()
        }
    }

    pub(super) fn op_paste_clips_at_playhead(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        if self.clipboard.clips.is_empty() || ctx.samples_per_beat <= 0.0 {
            return ArrangementAction {
                status: Some("Clipboard is empty".to_string()),
                ..Default::default()
            };
        }
        let entries = self.clipboard.clips.clone();
        let mut selected = HashSet::new();
        for entry in entries {
            match entry {
                ClipboardClip::Audio {
                    track_id,
                    offset_beats,
                    mut clip,
                } => {
                    if self.find_content(track_id).is_none() {
                        continue;
                    }
                    clip.id = ClipId::new();
                    clip.position = ctx.playhead_samples.saturating_add(
                        (offset_beats * ctx.samples_per_beat).round().max(0.0) as u64,
                    );
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
                    self.find_content_mut(track_id).unwrap().clips.push(clip);
                }
                ClipboardClip::Note {
                    track_id,
                    offset_beats,
                    mut clip,
                } => {
                    if self.find_content(track_id).is_none() {
                        continue;
                    }
                    clip.id = ClipId::new();
                    clip.position_beats = ctx.playhead_beats + offset_beats;
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
                    self.find_content_mut(track_id)
                        .unwrap()
                        .note_clips
                        .push(clip);
                }
            }
        }
        let count = selected.len();
        self.selected_clips = selected;
        self.time_selection_active = false;
        ArrangementAction {
            status: Some(if count == 1 {
                "Pasted clip".to_string()
            } else {
                format!("Pasted {count} clips")
            }),
            ..Default::default()
        }
    }

    pub(super) fn op_toggle_selected_clip_loop(
        &mut self,
        engine: &mut impl EngineHandle,
    ) -> ArrangementAction {
        let selections: Vec<_> = self.selected_clips.iter().copied().collect();
        let enable = selections.iter().any(|selection| match selection {
            ArrangementSelection::AudioClip { track_id, clip_id } => self
                .find_content(*track_id)
                .and_then(|t| t.clips.iter().find(|c| c.id == *clip_id))
                .is_some_and(|c| !c.loop_enabled),
            ArrangementSelection::NoteClip { track_id, clip_id } => self
                .find_content(*track_id)
                .and_then(|t| t.note_clips.iter().find(|c| c.id == *clip_id))
                .is_some_and(|c| !c.loop_enabled),
        });
        let mut changed = 0;
        for selection in selections {
            match selection {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    let mut command = None;
                    if let Some(clip) = self
                        .find_content_mut(track_id)
                        .and_then(|t| t.clips.iter_mut().find(|c| c.id == clip_id))
                    {
                        clip.loop_enabled = enable;
                        if enable && clip.loop_end <= clip.loop_start {
                            clip.loop_start = clip.source_offset;
                            clip.loop_end = clip.source_offset.saturating_add(clip.duration);
                        }
                        command = Some((clip.loop_start, clip.loop_end));
                        changed += 1;
                    }
                    if let Some((loop_start, loop_end)) = command {
                        engine.send(EngineCommand::SetClipLoop {
                            track_id,
                            clip_id,
                            enabled: enable,
                            loop_start,
                            loop_end,
                        });
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } => {
                    let mut command = None;
                    if let Some(clip) = self
                        .find_content_mut(track_id)
                        .and_then(|t| t.note_clips.iter_mut().find(|c| c.id == clip_id))
                    {
                        clip.loop_enabled = enable;
                        if enable && clip.loop_end_beats <= clip.loop_start_beats {
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        command = Some((clip.loop_start_beats, clip.loop_end_beats));
                        changed += 1;
                    }
                    if let Some((loop_start_beats, loop_end_beats)) = command {
                        engine.send(EngineCommand::SetNoteClipLoop {
                            track_id,
                            clip_id,
                            enabled: enable,
                            loop_start_beats,
                            loop_end_beats,
                        });
                    }
                }
            }
        }
        ArrangementAction {
            status: (changed > 0).then(|| {
                format!(
                    "{} loop for {changed} clip{}",
                    if enable { "Enabled" } else { "Disabled" },
                    if changed == 1 { "" } else { "s" }
                )
            }),
            ..Default::default()
        }
    }

    pub(super) fn op_resize_selected_clips(
        &mut self,
        _project_tracks: &mut ProjectTracksState,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        anchor: ArrangementSelection,
        new_duration_beats: f64,
    ) -> ArrangementAction {
        if ctx.samples_per_beat <= 0.0 {
            return ArrangementAction::default();
        }
        let anchor_duration = match anchor {
            ArrangementSelection::AudioClip { track_id, clip_id } => self
                .find_content(track_id)
                .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
                .map(|c| c.duration as f64 / ctx.samples_per_beat),
            ArrangementSelection::NoteClip { track_id, clip_id } => self
                .find_content(track_id)
                .and_then(|t| t.note_clips.iter().find(|c| c.id == clip_id))
                .map(|c| c.duration_beats),
        };
        let Some(anchor_duration) = anchor_duration else {
            return ArrangementAction::default();
        };
        let delta = new_duration_beats - anchor_duration;
        let targets: Vec<_> = if self.selected_clips.contains(&anchor) {
            self.selected_clips.iter().copied().collect()
        } else {
            vec![anchor]
        };
        let mut max_end = None::<f64>;
        for target in targets {
            match target {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    let old = self
                        .find_content(track_id)
                        .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
                        .map(|c| c.duration as f64 / ctx.samples_per_beat);
                    if let Some(old) = old {
                        let duration =
                            ((old + delta).max(0.25) * ctx.samples_per_beat).round() as u64;
                        let action = self.op_resize_audio_clip(
                            engine,
                            ctx,
                            track_id,
                            clip_id,
                            duration.max(1),
                        );
                        max_end = match (max_end, action.scroll_to_beat) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (None, value) | (value, None) => value,
                        };
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } => {
                    let mut sync = None;
                    if let Some(clip) = self
                        .find_content_mut(track_id)
                        .and_then(|t| t.note_clips.iter_mut().find(|c| c.id == clip_id))
                    {
                        clip.duration_beats = (clip.duration_beats + delta).max(0.25);
                        if clip.loop_enabled && clip.loop_end_beats > clip.duration_beats {
                            clip.loop_end_beats = clip.duration_beats;
                            if clip.loop_start_beats >= clip.loop_end_beats {
                                clip.loop_start_beats = 0.0;
                            }
                        }
                        sync = Some(clip.clone());
                        max_end = Some(
                            max_end
                                .unwrap_or(0.0)
                                .max(clip.position_beats + clip.duration_beats),
                        );
                    }
                    if let Some(clip) = sync {
                        engine.send(EngineCommand::RemoveNoteClip(track_id, clip_id));
                        engine.send(EngineCommand::AddNoteClip {
                            track_id,
                            clip_id,
                            position_beats: clip.position_beats,
                            duration_beats: clip.duration_beats,
                            loop_enabled: clip.loop_enabled,
                            loop_start_beats: clip.loop_start_beats,
                            loop_end_beats: clip.loop_end_beats,
                            groove_grid: clip.groove_grid,
                        });
                        for note in clip.notes {
                            engine.send(EngineCommand::AddNote {
                                track_id,
                                clip_id,
                                note,
                            });
                        }
                    }
                }
            }
        }
        self.drag_resize_active = true;
        ArrangementAction {
            scroll_to_beat: max_end,
            ..Default::default()
        }
    }
}
