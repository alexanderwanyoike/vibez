//! Whole-Clip Warp lifecycle and its composition with Warp Markers.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use crate::message::{AutoWarpOutcome, ClipWarpSuccess};

use super::audio_clip_inspector::clear_warp_request;
use super::{ArrangementAction, EngineHandle, TimelineEditorState};

impl TimelineEditorState {
    /// Install a completed whole-Clip Warp and scale its marker coordinates to
    /// the newly rendered buffer without changing their musical meaning.
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
        if let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|track| track.clips.iter_mut().find(|clip| clip.id == clip_id))
        {
            let source_ratio =
                success.audio.num_frames() as f64 / clip.audio.num_frames().max(1) as f64;
            let new_mapping_end = success
                .new_duration
                .min((success.audio.num_frames() as u64).saturating_sub(success.new_source_offset));
            let timeline_ratio = new_mapping_end as f64 / clip.warp_timeline_end().max(1) as f64;
            clip.transient_markers
                .scale_source_frames(source_ratio, success.audio.num_frames() as u64);
            clip.warp_markers.scale_frames(
                source_ratio,
                timeline_ratio,
                success.audio.num_frames() as u64,
                new_mapping_end,
            );
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
            engine.send(EngineCommand::SetClipWarpMarkers {
                track_id,
                clip_id,
                warp_markers: clip.warp_markers.clone(),
            });
        }
        self.discard_audio_clip_inspector_edits_for(clip_id);
        action.status = Some(format!("Warped to {:.0} BPM", success.warped_to_bpm));
        action.mark_dirty = true;
        action
    }

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
                action.status = Some(
                    "Auto-warp: could not detect BPM. Select the clip and type the source \
                     BPM in the Warp row, then press Enter and click Warp."
                        .to_string(),
                );
            }
            AutoWarpOutcome::DetectedOnly { bpm, confidence } => {
                if let Some(clip) = self
                    .find_content_mut(track_id)
                    .and_then(|track| track.clips.iter_mut().find(|clip| clip.id == clip_id))
                {
                    clip.original_bpm = Some(bpm);
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

    /// Clear both the piecewise map and any baked whole-Clip Warp.
    pub fn apply_clear_clip_warp(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let mut cleared_markers = false;
        let mut changed = false;
        if let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|track| track.clips.iter_mut().find(|clip| clip.id == clip_id))
        {
            let raw_timing_request = clip
                .original_audio
                .as_ref()
                .and_then(|_| clear_warp_request(track_id, clip));
            cleared_markers = clip.warp_markers.clear();
            changed = clip.warped || cleared_markers;
            clip.warped = false;
            clip.warped_to_bpm = None;
            if raw_timing_request.is_some() {
                action.transpose_render = raw_timing_request;
                action.status = Some("Returning Clip to raw timing...".into());
            } else {
                action.status = Some("Clip uses raw timing".into());
            }
        }
        if cleared_markers {
            engine.send(EngineCommand::SetClipWarpMarkers {
                track_id,
                clip_id,
                warp_markers: Default::default(),
            });
            self.selected_warp_marker = None;
        }
        action.mark_dirty = changed;
        action
    }
}
