//! Heavier arrangement operations: warp/quantize result
//! application and clip joining (audio-buffer merges).

use std::collections::HashSet;

use crate::message::{AudioQuantizeSuccess, AutoWarpOutcome, ClipWarpSuccess};
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{
    ArrangementSelection, ArrangementState, ClipClipboard, ClipboardClip, UiClip, UiNoteClip,
};

use super::*;

impl ArrangementState {
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

    pub(super) fn join_audio_clips(
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

    pub(super) fn join_note_clips(
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

    pub(super) fn op_split_audio_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        _ctx: ArrangementCtx,
        track_id: TrackId,
        clip_id: ClipId,
        split_position: u64,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let mut split_data = None;
        if let Some(track) = self.find_track(track_id) {
            if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                if split_position > clip.position && split_position < clip.position + clip.duration
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
        action
    }

    pub(super) fn op_split_note_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        _ctx: ArrangementCtx,
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
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
        if let Some((name, orig_pos, left_dur, right_pos, right_dur, left_notes, right_notes)) =
            split_data
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
        action
    }

    pub(super) fn op_delete_clips_in_region(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> ArrangementAction {
        let mut action = ArrangementAction {
            close_context_menu: true,
            ..Default::default()
        };
        let target_track = track_id;
        let spb = ctx.samples_per_beat;
        // Preserve material outside the range. Splitting first turns
        // the selected span into whole clips that can be removed.
        let _ = self.op_split_clips_at_region(engine, ctx, start_beats, end_beats, track_id);
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
                    if clip_start >= start_beats - 1e-9 && clip_end <= end_beats + 1e-9 {
                        audio_removals.push((track.id, clip.id));
                    }
                }
            }
            for nc in &track.note_clips {
                let clip_end = nc.position_beats + nc.duration_beats;
                if nc.position_beats >= start_beats - 1e-9 && clip_end <= end_beats + 1e-9 {
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
        let mut action = ArrangementAction {
            close_context_menu: true,
            ..Default::default()
        };
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
        action
    }

    pub(super) fn op_create_clip_from_selection(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if let Some(tid) = self.selected_track {
            if let Some(track) = self.find_track(tid) {
                if track.kind.is_midi() {
                    return self.update(
                        ArrangementMsg::CreateNoteClipFromSelection(tid),
                        engine,
                        ctx,
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
        engine: &mut impl EngineHandle,
        _ctx: ArrangementCtx,
        track_id: TrackId,
    ) -> ArrangementAction {
        let mut action = ArrangementAction {
            close_context_menu: true,
            ..Default::default()
        };
        if !self.time_selection_active || self.selection_end_beats <= self.selection_start_beats {
            action.status = Some("No time selection active".to_string());
            return action;
        }
        if let Some(track) = self.find_track(track_id) {
            if !track.kind.is_midi() {
                action.status = Some("Can only create note clips on MIDI tracks".to_string());
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
            for track in self
                .tracks
                .iter()
                .filter(|t| self.time_selection_track.is_none_or(|tid| t.id == tid))
            {
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
                        track_id: track.id,
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
                        track_id: track.id,
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
                            .find_track(*track_id)
                            .and_then(|t| t.clips.iter().find(|c| c.id == *clip_id))
                        {
                            starts.push(clip.position as f64 / spb);
                        }
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        if let Some(clip) = self
                            .find_track(*track_id)
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
                            .find_track(*track_id)
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
                            .find_track(*track_id)
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
                    if self.find_track(track_id).is_none() {
                        continue;
                    }
                    clip.id = ClipId::new();
                    clip.name = format!("{} (copy)", clip.name);
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
                    self.find_track_mut(track_id).unwrap().clips.push(clip);
                }
                ClipboardClip::Note {
                    track_id,
                    offset_beats,
                    mut clip,
                } => {
                    if self.find_track(track_id).is_none() {
                        continue;
                    }
                    clip.id = ClipId::new();
                    clip.name = format!("{} (copy)", clip.name);
                    clip.position_beats = ctx.playhead_beats + offset_beats;
                    engine.send(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: clip.id,
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
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
                    self.find_track_mut(track_id).unwrap().note_clips.push(clip);
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
                .find_track(*track_id)
                .and_then(|t| t.clips.iter().find(|c| c.id == *clip_id))
                .is_some_and(|c| !c.loop_enabled),
            ArrangementSelection::NoteClip { track_id, clip_id } => self
                .find_track(*track_id)
                .and_then(|t| t.note_clips.iter().find(|c| c.id == *clip_id))
                .is_some_and(|c| !c.loop_enabled),
        });
        let mut changed = 0;
        for selection in selections {
            match selection {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    let mut command = None;
                    if let Some(clip) = self
                        .find_track_mut(track_id)
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
                        .find_track_mut(track_id)
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
                .find_track(track_id)
                .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
                .map(|c| c.duration as f64 / ctx.samples_per_beat),
            ArrangementSelection::NoteClip { track_id, clip_id } => self
                .find_track(track_id)
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
                        .find_track(track_id)
                        .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
                        .map(|c| c.duration as f64 / ctx.samples_per_beat);
                    if let Some(old) = old {
                        let duration =
                            ((old + delta).max(0.25) * ctx.samples_per_beat).round() as u64;
                        let action = self.update(
                            ArrangementMsg::ResizeAudioClip {
                                track_id,
                                clip_id,
                                new_duration: duration.max(1),
                            },
                            engine,
                            ctx,
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
                        .find_track_mut(track_id)
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
