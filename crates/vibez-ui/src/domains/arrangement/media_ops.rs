//! Heavier arrangement operations: warp/quantize result
//! application and clip joining (audio-buffer merges).

use std::collections::HashSet;

use crate::message::{AudioQuantizeSuccess, AutoWarpOutcome, ClipWarpSuccess};
use std::sync::Arc;

use vibez_core::automation::AutomationTarget;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_engine::commands::EngineCommand;

use super::audio_clip_inspector::clear_warp_request;
use super::fragment_geometry::{audio_fragment_source_start, unmuted_beat_ranges, visible_notes};
use super::EngineHandle;
use crate::state::{
    ArrangementSelection, AudioClipInspectorField, TimelineEditorState, UiClip, UiNoteClip,
};

use super::*;

fn audio_source_frame(clip: &UiClip, local_frame: u64) -> usize {
    clip.source_frame_at(local_frame) as usize
}

impl TimelineEditorState {
    pub(super) fn op_trim_selected_by_track_mutes(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if self.selected_clips.is_empty() || ctx.samples_per_beat <= 0.0 {
            action.status = Some("Select clips to trim by Track Mutes".to_string());
            return action;
        }

        enum Replacement {
            Audio {
                track_id: TrackId,
                original_id: ClipId,
                clips: Vec<UiClip>,
            },
            Notes {
                track_id: TrackId,
                original_id: ClipId,
                clips: Vec<UiNoteClip>,
            },
        }

        let mut replacements = Vec::new();
        for selection in self.selected_clips.iter().copied() {
            let (track_id, clip_id) = match selection {
                ArrangementSelection::AudioClip { track_id, clip_id }
                | ArrangementSelection::NoteClip { track_id, clip_id } => (track_id, clip_id),
            };
            let Some(content) = self.find_content(track_id) else {
                continue;
            };
            let Some(mute_lane) = content
                .automation
                .iter()
                .find(|lane| lane.target == AutomationTarget::TrackMute)
            else {
                continue;
            };

            match selection {
                ArrangementSelection::AudioClip { .. } => {
                    let Some(clip) = content.clips.iter().find(|clip| clip.id == clip_id) else {
                        continue;
                    };
                    let clip_start_beat = clip.position as f64 / ctx.samples_per_beat;
                    let clip_end_sample = clip.position.saturating_add(clip.duration);
                    let clip_end_beat = clip_end_sample as f64 / ctx.samples_per_beat;
                    let ranges = unmuted_beat_ranges(mute_lane, clip_start_beat, clip_end_beat);
                    let covers_original = ranges.as_slice() == [(clip_start_beat, clip_end_beat)];
                    if covers_original {
                        continue;
                    }
                    let clips = ranges
                        .into_iter()
                        .filter_map(|(start, end)| {
                            let start_sample = ((start * ctx.samples_per_beat).round() as u64)
                                .clamp(clip.position, clip_end_sample);
                            let end_sample = ((end * ctx.samples_per_beat).round() as u64)
                                .clamp(start_sample, clip_end_sample);
                            (end_sample > start_sample).then(|| {
                                let local_start = start_sample - clip.position;
                                let mut fragment = clip.clone();
                                fragment.id = ClipId::new();
                                fragment.position = start_sample;
                                fragment.source_offset = audio_fragment_source_start(
                                    clip,
                                    local_start,
                                    end_sample - start_sample,
                                );
                                fragment.start_marker = fragment.source_offset;
                                fragment.duration = end_sample - start_sample;
                                fragment.fades = clip.fades.for_fragment(
                                    clip.duration,
                                    local_start,
                                    fragment.duration,
                                );
                                fragment.transient_markers.retain_source_range(
                                    fragment.source_offset,
                                    fragment.source_offset.saturating_add(fragment.duration),
                                );
                                fragment
                            })
                        })
                        .collect();
                    replacements.push(Replacement::Audio {
                        track_id,
                        original_id: clip_id,
                        clips,
                    });
                }
                ArrangementSelection::NoteClip { .. } => {
                    let Some(clip) = content.note_clips.iter().find(|clip| clip.id == clip_id)
                    else {
                        continue;
                    };
                    let clip_end = clip.position_beats + clip.duration_beats;
                    let ranges = unmuted_beat_ranges(mute_lane, clip.position_beats, clip_end);
                    let covers_original = ranges.as_slice() == [(clip.position_beats, clip_end)];
                    if covers_original {
                        continue;
                    }
                    let clips = ranges
                        .into_iter()
                        .map(|(start, end)| {
                            let local_start = start - clip.position_beats;
                            let local_end = end - clip.position_beats;
                            let mut fragment = clip.clone();
                            fragment.id = ClipId::new();
                            fragment.position_beats = start;
                            fragment.duration_beats = end - start;
                            fragment.notes = visible_notes(clip, local_start, local_end);
                            fragment.selected_notes.clear();
                            fragment.start_marker_beats = 0.0;
                            if fragment.loop_enabled {
                                fragment.reset_loop_region_to_clip();
                            }
                            fragment
                        })
                        .collect();
                    replacements.push(Replacement::Notes {
                        track_id,
                        original_id: clip_id,
                        clips,
                    });
                }
            }
        }

        if replacements.is_empty() {
            action.status = Some("No selected clip material overlaps Track Mutes".to_string());
            return action;
        }

        let trimmed_count = replacements.len();
        let mut fragment_count = 0;
        let mut new_selection = self.selected_clips.clone();
        for replacement in replacements {
            match replacement {
                Replacement::Audio {
                    track_id,
                    original_id,
                    clips,
                } => {
                    new_selection.remove(&ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: original_id,
                    });
                    for clip in &clips {
                        new_selection.insert(ArrangementSelection::AudioClip {
                            track_id,
                            clip_id: clip.id,
                        });
                    }
                    fragment_count += clips.len();
                    self.replace_audio_clip(engine, track_id, original_id, clips);
                }
                Replacement::Notes {
                    track_id,
                    original_id,
                    clips,
                } => {
                    new_selection.remove(&ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: original_id,
                    });
                    for clip in &clips {
                        new_selection.insert(ArrangementSelection::NoteClip {
                            track_id,
                            clip_id: clip.id,
                        });
                    }
                    fragment_count += clips.len();
                    self.replace_note_clip(engine, track_id, original_id, clips);
                }
            }
        }

        self.selected_clips = new_selection;
        self.selected_note_clip =
            self.selected_clips
                .iter()
                .find_map(|selection| match selection {
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        Some((*track_id, *clip_id))
                    }
                    ArrangementSelection::AudioClip { .. } => None,
                });
        action.status = Some(format!(
            "Trimmed {trimmed_count} clip{} by Track Mutes · kept {fragment_count} fragment{}",
            if trimmed_count == 1 { "" } else { "s" },
            if fragment_count == 1 { "" } else { "s" },
        ));
        action
    }

    pub(super) fn op_resize_audio_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        track_id: TrackId,
        clip_id: ClipId,
        new_duration: u64,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        self.unlink_crossfades_for_clip(engine, track_id, clip_id);
        let mut sync_data = None;
        let mut clip_end_beat = None;
        if let Some(track) = self.find_content_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                clip.duration = new_duration;
                clip.clamp_fades_to_clip();
                clip.clamp_start_to_source();
                if clip.loop_enabled {
                    clip.clamp_loop_to_clip();
                }
                clip.transient_markers.retain_source_range(
                    clip.source_offset,
                    clip.source_offset.saturating_add(clip.duration),
                );
                clip_end_beat = Some((clip.position + clip.duration) as f64 / ctx.samples_per_beat);
                sync_data = Some((
                    Arc::clone(&clip.audio),
                    clip.position,
                    clip.source_offset,
                    clip.start_marker,
                    clip.duration,
                    clip.loop_enabled,
                    clip.loop_start,
                    clip.loop_end,
                    clip.gain_db.linear(),
                    clip.fades,
                    clip.playback_direction,
                ));
            }
        }
        if let Some((
            audio,
            position,
            source_offset,
            start_marker,
            duration,
            loop_enabled,
            loop_start,
            loop_end,
            linear_gain,
            fades,
            playback_direction,
        )) = sync_data
        {
            engine.send(EngineCommand::RemoveClip(track_id, clip_id));
            engine.send(EngineCommand::AddClip {
                track_id,
                clip_id,
                audio,
                position,
                source_offset,
                start_marker,
                duration,
                loop_enabled,
                loop_start,
                loop_end,
                linear_gain,
                fades,
                playback_direction,
            });
        }
        action.scroll_to_beat = clip_end_beat;
        self.drag_resize_active = true;
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
        self.unlink_crossfades_for_clip(engine, track_id, clip_id);
        engine.send(EngineCommand::ReplaceClipAudio {
            track_id,
            clip_id,
            audio: Arc::clone(&success.audio),
            duration: success.new_duration,
            source_offset: success.new_source_offset,
            start_marker: success.new_start_marker,
            loop_start: success.new_loop_start,
            loop_end: success.new_loop_end,
        });
        if let Some(track) = self.find_content_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                let marker_ratio =
                    success.audio.num_frames() as f64 / clip.audio.num_frames().max(1) as f64;
                clip.transient_markers
                    .scale_source_frames(marker_ratio, success.audio.num_frames() as u64);
                clip.fades = clip.fades.scaled(clip.duration, success.new_duration);
                clip.audio = Arc::clone(&success.audio);
                clip.duration = success.new_duration;
                clip.source_offset = success.new_source_offset;
                clip.start_marker = success.new_start_marker;
                clip.loop_start = success.new_loop_start;
                clip.loop_end = success.new_loop_end;
                clip.original_bpm = Some(success.detected_bpm);
                clip.warped = true;
                clip.warped_to_bpm = Some(success.warped_to_bpm);
                clip.original_audio = Some(Arc::clone(&success.original_audio));
                engine.send(EngineCommand::SetClipFades {
                    track_id,
                    clip_id,
                    fades: clip.fades,
                });
            }
        }
        self.discard_audio_clip_inspector_edits_for(clip_id);
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
                if let Some(track) = self.find_content_mut(track_id) {
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
        _engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|track| track.clips.iter_mut().find(|clip| clip.id == clip_id))
        {
            if clip.original_audio.is_some() {
                clip.warped = false;
                clip.warped_to_bpm = None;
                action.transpose_render = clear_warp_request(track_id, clip);
                action.status = Some("Returning Clip to raw timing...".into());
            } else {
                clip.warped = false;
                clip.warped_to_bpm = None;
                action.status = Some("Clip uses raw timing".into());
            }
        }
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
        let gain_db = self
            .find_content(track_id)
            .and_then(|track| track.clips.iter().find(|clip| clip.id == old_clip_id))
            .map(|clip| clip.gain_db)
            .unwrap_or_default();
        self.unlink_crossfades_for_clip(engine, track_id, old_clip_id);
        engine.send(EngineCommand::RemoveClip(track_id, old_clip_id));
        if let Some(track) = self.find_content_mut(track_id) {
            track.clips.retain(|c| c.id != old_clip_id);
        }
        self.selected_clips.retain(|sel| match sel {
            ArrangementSelection::AudioClip {
                clip_id: cid,
                track_id: tid,
            } => !(*tid == track_id && *cid == old_clip_id),
            _ => true,
        });
        self.discard_audio_clip_inspector_edits_for(old_clip_id);

        engine.send(EngineCommand::AddClip {
            track_id,
            clip_id: success.new_clip_id,
            audio: Arc::clone(&success.new_audio),
            position: success.new_position,
            source_offset: 0,
            start_marker: 0,
            duration: success.new_duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            linear_gain: gain_db.linear(),
            fades: Default::default(),
            playback_direction: Default::default(),
        });
        if let Some(track) = self.find_content_mut(track_id) {
            track.clips.push(UiClip {
                id: success.new_clip_id,
                name: success.new_name,
                audio: Arc::clone(&success.new_audio),
                source: None,
                position: success.new_position,
                source_offset: 0,
                start_marker: 0,
                duration: success.new_duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                gain_db,
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                transpose: Default::default(),
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
        action.mark_dirty = true;
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
                if let Some(track) = self.find_content_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(b);
                        if clip.warped {
                            action.warp_refresh = Some((track_id, clip_id));
                        }
                    }
                }
                self.audio_clip_inspector_edits
                    .remove(&(clip_id, AudioClipInspectorField::SourceBpm));
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

        let mut clip_data: Vec<UiClip> = Vec::new();
        if let Some(track) = self.find_content(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.clips.iter().find(|c| c.id == *cid) {
                    clip_data.push(clip.clone());
                }
            }
        }

        if clip_data.len() < 2 {
            return None;
        }

        // Sort by position
        clip_data.sort_by_key(|clip| clip.position);

        let start_pos = clip_data[0].position;
        let end_pos = clip_data
            .iter()
            .map(|clip| clip.position.saturating_add(clip.duration))
            .max()
            .unwrap_or(start_pos);
        let total_duration = end_pos - start_pos;
        let joined_loop_enabled = clip_data.iter().any(|clip| clip.loop_enabled);

        // Determine channel count from first clip
        let channels = clip_data[0].audio.num_channels();
        let sr = clip_data[0].audio.sample_rate;

        // Create joined buffer filled with silence
        let mut joined_channels: Vec<Vec<f32>> = (0..channels)
            .map(|_| vec![0.0f32; total_duration as usize])
            .collect();

        // Consolidate the audible arrangement result, including source
        // wrapping for clips whose visible duration exceeds their loop.
        for clip in &clip_data {
            let offset_in_joined = (clip.position - start_pos) as usize;
            let dur = clip.duration as usize;
            let ch_count = channels.min(clip.audio.num_channels());
            for (ch, dst) in joined_channels.iter_mut().enumerate().take(ch_count) {
                for local in 0..dur {
                    let dst_frame = offset_in_joined + local;
                    if dst_frame >= dst.len() {
                        break;
                    }
                    let source_frame = audio_source_frame(clip, local as u64);
                    if let Some(sample) = clip.audio.channels[ch].get(source_frame) {
                        dst[dst_frame] += *sample
                            * clip.gain_db.linear()
                            * clip.fades.gain_at(local as u64, clip.duration);
                    }
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
            self.unlink_crossfades_for_clip(engine, track_id, *cid);
            engine.send(EngineCommand::RemoveClip(track_id, *cid));
            if let Some(track) = self.find_content_mut(track_id) {
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
            start_marker: 0,
            duration: total_duration,
            loop_enabled: joined_loop_enabled,
            loop_start: 0,
            loop_end: total_duration,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
        });
        if let Some(track) = self.find_content_mut(track_id) {
            track.clips.push(UiClip {
                id: new_id,
                name: "Joined".to_string(),
                audio: joined_audio,
                source: None,
                position: start_pos,
                source_offset: 0,
                start_marker: 0,
                duration: total_duration,
                loop_enabled: joined_loop_enabled,
                loop_start: 0,
                loop_end: total_duration,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                transpose: Default::default(),
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

        let mut clip_data: Vec<UiNoteClip> = Vec::new();
        if let Some(track) = self.find_content(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.note_clips.iter().find(|c| c.id == *cid) {
                    clip_data.push(clip.clone());
                }
            }
        }

        if clip_data.len() < 2 {
            return None;
        }

        // Sort by position
        clip_data.sort_by(|a, b| {
            a.position_beats
                .partial_cmp(&b.position_beats)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let start_pos = clip_data[0].position_beats;
        let end_pos = clip_data
            .iter()
            .map(|clip| clip.position_beats + clip.duration_beats)
            .fold(0.0_f64, f64::max);
        let total_duration = end_pos - start_pos;
        let joined_loop_enabled = clip_data.iter().any(|clip| clip.loop_enabled);

        // Merge the audible notes, expanding repeated loop occurrences.
        let mut merged_notes: Vec<MidiNote> = Vec::new();
        for clip in &clip_data {
            let offset = clip.position_beats - start_pos;
            for note in visible_notes(clip, 0.0, clip.duration_beats) {
                merged_notes.push(MidiNote {
                    start_beat: note.start_beat + offset,
                    ..note
                });
            }
        }

        // Remove all originals
        for cid in &clip_ids {
            engine.send(EngineCommand::RemoveNoteClip(track_id, *cid));
            if let Some(track) = self.find_content_mut(track_id) {
                track.note_clips.retain(|c| c.id != *cid);
            }
        }

        // Add joined clip
        let new_id = ClipId::new();
        engine.send(EngineCommand::AddNoteClip {
            start_marker_beats: 0.0,
            track_id,
            clip_id: new_id,
            position_beats: start_pos,
            duration_beats: total_duration,
            loop_enabled: joined_loop_enabled,
            loop_start_beats: 0.0,
            loop_end_beats: total_duration,
            groove_grid: vibez_core::perform::GrooveGrid::Off,
        });
        for note in &merged_notes {
            engine.send(EngineCommand::AddNote {
                track_id,
                clip_id: new_id,
                note: *note,
            });
        }
        if let Some(track) = self.find_content_mut(track_id) {
            track.note_clips.push(UiNoteClip {
                id: new_id,
                name: "Joined".to_string(),
                position_beats: start_pos,
                duration_beats: total_duration,
                notes: merged_notes,
                selected_notes: HashSet::new(),
                start_marker_beats: 0.0,
                loop_enabled: joined_loop_enabled,
                loop_start_beats: 0.0,
                loop_end_beats: total_duration,
                groove_grid: vibez_core::perform::GrooveGrid::Off,
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

    /// Replace `original_id` on `track_id` with `fragments` in both the
    /// engine and UI state. Every op that fragments a clip (split, trim) must
    /// route through here so the engine command list stays in one place;
    /// selection bookkeeping stays with the caller.
    fn replace_audio_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        original_id: ClipId,
        fragments: Vec<UiClip>,
    ) {
        self.unlink_crossfades_for_clip(engine, track_id, original_id);
        engine.send(EngineCommand::RemoveClip(track_id, original_id));
        if let Some(content) = self.find_content_mut(track_id) {
            content.clips.retain(|clip| clip.id != original_id);
        }
        for clip in &fragments {
            engine.send(EngineCommand::AddClip {
                track_id,
                clip_id: clip.id,
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
            });
        }
        if let Some(content) = self.find_content_mut(track_id) {
            content.clips.extend(fragments);
        }
    }

    /// Note-clip counterpart of [`Self::replace_audio_clip`].
    fn replace_note_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        original_id: ClipId,
        fragments: Vec<UiNoteClip>,
    ) {
        engine.send(EngineCommand::RemoveNoteClip(track_id, original_id));
        if let Some(content) = self.find_content_mut(track_id) {
            content.note_clips.retain(|clip| clip.id != original_id);
        }
        for clip in &fragments {
            engine.send(EngineCommand::AddNoteClip {
                start_marker_beats: clip.start_marker_beats,
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
        }
        if let Some(content) = self.find_content_mut(track_id) {
            content.note_clips.extend(fragments);
        }
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
        let split = self
            .find_content(track_id)
            .and_then(|track| track.clips.iter().find(|clip| clip.id == clip_id))
            .filter(|clip| {
                split_position > clip.position
                    && split_position < clip.position.saturating_add(clip.duration)
            })
            .map(|clip| {
                let left_duration = split_position - clip.position;
                let mut left = clip.clone();
                left.id = ClipId::new();
                left.name = format!("{} L", clip.name);
                left.source_offset = audio_fragment_source_start(clip, 0, left_duration);
                left.start_marker = left.source_offset;
                left.duration = left_duration;
                left.fades = clip.fades.for_fragment(clip.duration, 0, left.duration);
                left.transient_markers.retain_source_range(
                    left.source_offset,
                    left.source_offset.saturating_add(left.duration),
                );

                let mut right = clip.clone();
                right.id = ClipId::new();
                right.name = format!("{} R", clip.name);
                right.position = split_position;
                right.duration = clip.duration - left_duration;
                right.fades = clip
                    .fades
                    .for_fragment(clip.duration, left_duration, right.duration);
                right.source_offset =
                    audio_fragment_source_start(clip, left_duration, right.duration);
                right.start_marker = right.source_offset;
                right.transient_markers.retain_source_range(
                    right.source_offset,
                    right.source_offset.saturating_add(right.duration),
                );
                (left, right)
            });
        if let Some((left, right)) = split {
            let left_id = left.id;
            self.replace_audio_clip(engine, track_id, clip_id, vec![left, right]);

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
        let split = self
            .find_content(track_id)
            .and_then(|track| track.note_clips.iter().find(|clip| clip.id == clip_id))
            .filter(|clip| {
                split_beat > clip.position_beats
                    && split_beat < clip.position_beats + clip.duration_beats
            })
            .map(|clip| {
                let local_split = split_beat - clip.position_beats;
                let mut left = clip.clone();
                left.id = ClipId::new();
                left.name = format!("{} L", clip.name);
                left.duration_beats = local_split;
                left.notes = visible_notes(clip, 0.0, local_split);
                left.selected_notes.clear();
                left.start_marker_beats = 0.0;

                let mut right = clip.clone();
                right.id = ClipId::new();
                right.name = format!("{} R", clip.name);
                right.position_beats = split_beat;
                right.duration_beats = clip.duration_beats - local_split;
                right.notes = visible_notes(clip, local_split, clip.duration_beats);
                right.selected_notes.clear();
                right.start_marker_beats = 0.0;

                if clip.loop_enabled {
                    left.reset_loop_region_to_clip();
                    right.reset_loop_region_to_clip();
                }
                (left, right)
            });
        if let Some((left, right)) = split {
            let left_id = left.id;
            self.replace_note_clip(engine, track_id, clip_id, vec![left, right]);

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
}
