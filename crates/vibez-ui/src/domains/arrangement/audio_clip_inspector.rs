//! Audio Clip Inspector edits and asynchronous Transpose render lifecycle.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::{ClipGainDb, ClipTranspose};
use vibez_engine::commands::EngineCommand;

use crate::message::ClipTransposeSuccess;
use crate::state::{AudioClipInspectorField, AudioClipRotaryField, TimelineEditorState, UiClip};

use super::{ArrangementAction, ClipRenderedGeometry, ClipTransposeRenderRequest, EngineHandle};

fn rendered_geometry(clip: &UiClip) -> ClipRenderedGeometry {
    ClipRenderedGeometry {
        source_offset: clip.source_offset,
        start_marker: clip.start_marker,
        duration: clip.duration,
        loop_start: clip.loop_start,
        loop_end: clip.loop_end,
    }
}

fn transpose_request(track_id: TrackId, clip: &UiClip) -> ClipTransposeRenderRequest {
    let source_audio = clip
        .original_audio
        .clone()
        .unwrap_or_else(|| Arc::clone(&clip.audio));
    ClipTransposeRenderRequest {
        track_id,
        clip_id: clip.id,
        source_audio,
        target_frames: clip.audio.num_frames(),
        transpose: clip.transpose,
        expected_warped: clip.warped,
        expected_audio: Arc::clone(&clip.audio),
        expected_geometry: None,
        geometry: None,
    }
}

pub(super) fn clear_warp_request(
    track_id: TrackId,
    clip: &UiClip,
) -> Option<ClipTransposeRenderRequest> {
    let original = clip.original_audio.as_ref().map(Arc::clone)?;
    let current_frames = clip.audio.num_frames().max(1) as f64;
    let original_frames = original.num_frames() as u64;
    let ratio = original_frames as f64 / current_frames;
    let scale = |frames: u64| (frames as f64 * ratio).round() as u64;
    let source_offset = scale(clip.source_offset).min(original_frames);
    let source_end = scale(clip.source_end())
        .min(original_frames)
        .max(source_offset);
    Some(ClipTransposeRenderRequest {
        track_id,
        clip_id: clip.id,
        source_audio: original,
        target_frames: original_frames as usize,
        transpose: clip.transpose,
        expected_warped: false,
        expected_audio: Arc::clone(&clip.audio),
        expected_geometry: Some(rendered_geometry(clip)),
        geometry: Some(ClipRenderedGeometry {
            source_offset,
            start_marker: scale(clip.start_marker).min(original_frames.saturating_sub(1)),
            duration: source_end - source_offset,
            loop_start: scale(clip.loop_start).min(original_frames),
            loop_end: scale(clip.loop_end).min(original_frames),
        }),
    })
}

impl TimelineEditorState {
    pub fn apply_clip_transpose_success(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        success: ClipTransposeSuccess,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let Some(clip) = self
            .find_content(track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
        else {
            return action;
        };
        if clip.transpose != success.transpose {
            return action;
        }
        let stale_audio = !Arc::ptr_eq(&clip.audio, &success.expected_audio);
        let stale_geometry = success
            .expected_geometry
            .is_some_and(|expected| rendered_geometry(clip) != expected);
        if clip.warped != success.expected_warped || stale_audio || stale_geometry {
            action.transpose_render = if success.geometry.is_some() && !clip.warped {
                clear_warp_request(track_id, clip)
            } else {
                Some(transpose_request(track_id, clip))
            };
            action.status = Some("Refreshing Clip render after a newer edit...".into());
            return action;
        }
        if success.geometry.is_some() {
            self.unlink_crossfades_for_clip(engine, track_id, clip_id);
        }
        let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
        else {
            return action;
        };
        let previous_audio_frames = clip.audio.num_frames().max(1);
        clip.audio = Arc::clone(&success.audio);
        let replaces_geometry = success.geometry.is_some();
        if let Some(geometry) = success.geometry {
            let marker_ratio = success.audio.num_frames() as f64 / previous_audio_frames as f64;
            clip.transient_markers
                .scale_source_frames(marker_ratio, success.audio.num_frames() as u64);
            clip.fades = clip.fades.scaled(clip.duration, geometry.duration);
            clip.source_offset = geometry.source_offset;
            clip.start_marker = geometry.start_marker;
            clip.duration = geometry.duration;
            clip.loop_start = geometry.loop_start;
            clip.loop_end = geometry.loop_end;
            engine.send(EngineCommand::ReplaceClipAudio {
                track_id,
                clip_id,
                audio: Arc::clone(&success.audio),
                duration: geometry.duration,
                source_offset: geometry.source_offset,
                start_marker: geometry.start_marker,
                loop_start: geometry.loop_start,
                loop_end: geometry.loop_end,
            });
            engine.send(EngineCommand::SetClipFades {
                track_id,
                clip_id,
                fades: clip.fades,
            });
        } else {
            engine.send(EngineCommand::ReplaceClipBuffer {
                track_id,
                clip_id,
                audio: Arc::clone(&success.audio),
            });
        }
        clip.original_audio = if clip.transpose.semitones() == 0 && !clip.warped {
            None
        } else {
            Some(success.source_audio)
        };
        action.status = success.warning.or_else(|| {
            Some(format!(
                "Clip Transpose {:+} st",
                clip.transpose.semitones()
            ))
        });
        if replaces_geometry {
            self.discard_audio_clip_inspector_edits_for(clip_id);
        }
        action
    }

    pub(super) fn commit_audio_clip_inspector_field(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipInspectorField,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let Some(text) = self
            .audio_clip_inspector_edits
            .get(&(clip_id, field))
            .cloned()
        else {
            return action;
        };
        match field {
            AudioClipInspectorField::FadeIn => self.unlink_crossfade_edge_for_clip(
                engine,
                track_id,
                clip_id,
                crate::state::AudioClipFadeEdge::In,
            ),
            AudioClipInspectorField::FadeOut => self.unlink_crossfade_edge_for_clip(
                engine,
                track_id,
                clip_id,
                crate::state::AudioClipFadeEdge::Out,
            ),
            AudioClipInspectorField::SourceStart | AudioClipInspectorField::SourceEnd => {
                self.unlink_crossfades_for_clip(engine, track_id, clip_id);
            }
            _ => {}
        }
        let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
        else {
            self.discard_audio_clip_inspector_edits_for(clip_id);
            action.status = Some("Audio Clip is no longer available".into());
            return action;
        };

        let source_frames = clip.audio.num_frames() as u64;
        let sample_rate = u64::from(clip.audio.sample_rate.max(1));
        let seconds_to_frames = |seconds: f64| -> Option<u64> {
            (seconds.is_finite() && seconds >= 0.0)
                .then(|| (seconds * sample_rate as f64).round() as u64)
        };
        let format_seconds = |frames: u64| frames as f64 / sample_rate as f64;

        match field {
            AudioClipInspectorField::Gain => {
                let Some(gain) = text.parse::<f32>().ok().and_then(ClipGainDb::new) else {
                    action.status = Some(format!(
                        "Clip Gain must be {:.0} to +{:.0} dB",
                        ClipGainDb::MIN,
                        ClipGainDb::MAX
                    ));
                    return action;
                };
                clip.gain_db = gain;
                engine.send(EngineCommand::SetClipGain {
                    track_id,
                    clip_id,
                    linear_gain: gain.linear(),
                });
                action.status = Some(format!("Clip Gain {:+.1} dB", gain.db()));
            }
            AudioClipInspectorField::FadeIn | AudioClipInspectorField::FadeOut => {
                let Some(frames) = text.parse::<f64>().ok().and_then(seconds_to_frames) else {
                    action.status = Some("Fade length must be a positive time in seconds".into());
                    return action;
                };
                let fades = match field {
                    AudioClipInspectorField::FadeIn => {
                        clip.fades.with_fade_in(frames, clip.duration)
                    }
                    AudioClipInspectorField::FadeOut => {
                        clip.fades.with_fade_out(frames, clip.duration)
                    }
                    _ => unreachable!(),
                };
                clip.fades = fades;
                engine.send(EngineCommand::SetClipFades {
                    track_id,
                    clip_id,
                    fades,
                });
                action.status = Some(format!(
                    "Clip fades {:.3} s in, {:.3} s out",
                    format_seconds(fades.fade_in_frames()),
                    format_seconds(fades.fade_out_frames())
                ));
            }
            AudioClipInspectorField::SourceBpm => {
                let Some(bpm) = text
                    .parse::<f64>()
                    .ok()
                    .filter(|bpm| bpm.is_finite() && *bpm > 0.0 && *bpm < 1_000.0)
                else {
                    action.status = Some("Source BPM must be between 0 and 1000".into());
                    return action;
                };
                clip.original_bpm = Some(bpm);
                if clip.warped {
                    action.warp_refresh = Some((track_id, clip_id));
                }
                action.status = Some(format!("Source tempo {bpm:.1} BPM"));
            }
            AudioClipInspectorField::SourceStart | AudioClipInspectorField::SourceEnd => {
                let Some(value) = text.parse::<f64>().ok().and_then(seconds_to_frames) else {
                    action.status =
                        Some("Source boundary must be a positive time in seconds".into());
                    return action;
                };
                let current_end = clip.source_end().min(source_frames);
                let (new_start, new_end) = match field {
                    AudioClipInspectorField::SourceStart => (value, current_end),
                    AudioClipInspectorField::SourceEnd => (clip.source_offset, value),
                    _ => unreachable!(),
                };
                if new_start >= new_end || new_end > source_frames {
                    action.status = Some(format!(
                        "Source bounds must stay ordered inside 0.000 to {:.3} s",
                        format_seconds(source_frames)
                    ));
                    return action;
                }
                clip.source_offset = new_start;
                clip.duration = new_end - new_start;
                let cleared_warp_markers = clip.warp_markers.clear();
                clip.transient_markers
                    .retain_source_range(new_start, new_end);
                clip.clamp_fades_to_clip();
                clip.clamp_start_to_source();
                clip.loop_start = clip.loop_start.clamp(new_start, new_end);
                clip.loop_end = clip.loop_end.clamp(clip.loop_start, new_end);
                if clip.loop_enabled
                    && (clip.loop_end <= clip.loop_start || clip.start_marker >= clip.loop_end)
                {
                    clip.reset_loop_region_to_clip();
                }
                engine.send(EngineCommand::SetClipBounds {
                    track_id,
                    clip_id,
                    source_offset: clip.source_offset,
                    start_marker: clip.start_marker,
                    duration: clip.duration,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                });
                engine.send(EngineCommand::SetClipFades {
                    track_id,
                    clip_id,
                    fades: clip.fades,
                });
                if cleared_warp_markers {
                    engine.send(EngineCommand::SetClipWarpMarkers {
                        track_id,
                        clip_id,
                        warp_markers: Default::default(),
                    });
                }
                engine.send(EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled: clip.loop_enabled,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                });
                action.status = Some(if cleared_warp_markers {
                    format!(
                        "Source {:.3} to {:.3} s · Warp Markers cleared",
                        format_seconds(new_start),
                        format_seconds(new_end)
                    )
                } else {
                    format!(
                        "Source {:.3} to {:.3} s",
                        format_seconds(new_start),
                        format_seconds(new_end)
                    )
                });
                if cleared_warp_markers {
                    self.selected_warp_marker = None;
                }
            }
            AudioClipInspectorField::Start => {
                let Some(value) = text.parse::<f64>().ok().and_then(seconds_to_frames) else {
                    action.status = Some("Start must be a positive time in seconds".into());
                    return action;
                };
                if !clip.set_start_marker(value) {
                    let source_end = clip.source_offset.saturating_add(clip.duration);
                    action.status = Some(format!(
                        "Start must be inside {:.3} to {:.3} s and before Loop End",
                        format_seconds(clip.source_offset),
                        format_seconds(source_end)
                    ));
                    return action;
                }
                engine.send(EngineCommand::SetClipStartMarker {
                    track_id,
                    clip_id,
                    start_marker: value,
                });
                action.status = Some(format!("Clip Start {:.3} s", format_seconds(value)));
            }
            AudioClipInspectorField::LoopStart | AudioClipInspectorField::LoopEnd => {
                let Some(value) = text.parse::<f64>().ok().and_then(seconds_to_frames) else {
                    action.status = Some("Loop boundary must be a positive time in seconds".into());
                    return action;
                };
                let source_end = clip.source_offset.saturating_add(clip.duration);
                let (new_start, new_end) = match field {
                    AudioClipInspectorField::LoopStart => {
                        let end = if clip.loop_end > value && clip.loop_end <= source_end {
                            clip.loop_end
                        } else {
                            source_end
                        };
                        (value, end)
                    }
                    AudioClipInspectorField::LoopEnd => {
                        let start =
                            if clip.loop_start >= clip.source_offset && clip.loop_start < value {
                                clip.loop_start
                            } else {
                                clip.source_offset
                            };
                        (start, value)
                    }
                    _ => unreachable!(),
                };
                if new_start < clip.source_offset
                    || new_start >= new_end
                    || new_end > source_end
                    || clip.start_marker >= new_end
                {
                    action.status = Some(format!(
                        "Loop bounds must stay ordered inside {:.3} to {:.3} s",
                        format_seconds(clip.source_offset),
                        format_seconds(source_end)
                    ));
                    return action;
                }
                clip.loop_start = new_start;
                clip.loop_end = new_end;
                engine.send(EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled: clip.loop_enabled,
                    loop_start: new_start,
                    loop_end: new_end,
                });
                action.status = Some(format!(
                    "Loop {:.3} to {:.3} s",
                    format_seconds(new_start),
                    format_seconds(new_end)
                ));
            }
            AudioClipInspectorField::Transpose => {
                let Some(semitones) = text
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(|value| value.round())
                    .filter(|value| {
                        (f64::from(ClipTranspose::MIN)..=f64::from(ClipTranspose::MAX))
                            .contains(value)
                    })
                    .map(|value| value as i8)
                else {
                    action.status = Some(format!(
                        "Transpose must be {} to +{} semitones",
                        ClipTranspose::MIN,
                        ClipTranspose::MAX
                    ));
                    return action;
                };
                clip.transpose = ClipTranspose::new(semitones);
                let request = transpose_request(track_id, clip);
                clip.original_audio = Some(Arc::clone(&request.source_audio));
                action.transpose_render = Some(request);
                action.status = Some(format!("Transposing Clip {semitones:+} st..."));
            }
        }
        self.audio_clip_inspector_edits.remove(&(clip_id, field));
        if field == AudioClipInspectorField::Transpose {
            self.audio_clip_transpose_debounce.remove(&clip_id);
        }
        action.mark_dirty = true;
        action
    }

    pub(super) fn set_audio_clip_rotary_value(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipRotaryField,
        value: f32,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
        else {
            action.status = Some("Audio Clip is no longer available".into());
            return action;
        };
        match field {
            AudioClipRotaryField::Gain => {
                let Some(gain) = ClipGainDb::new(value) else {
                    return action;
                };
                clip.gain_db = gain;
                engine.send(EngineCommand::SetClipGain {
                    track_id,
                    clip_id,
                    linear_gain: gain.linear(),
                });
                action.status = Some(format!("Clip Gain {:+.1} dB", gain.db()));
            }
            AudioClipRotaryField::Transpose => {
                let semitones = value
                    .round()
                    .clamp(f32::from(ClipTranspose::MIN), f32::from(ClipTranspose::MAX))
                    as i8;
                clip.transpose = ClipTranspose::new(semitones);
                let request = transpose_request(track_id, clip);
                clip.original_audio = Some(Arc::clone(&request.source_audio));
                action.transpose_render = Some(request);
                action.status = Some(format!("Transposing Clip {semitones:+} st..."));
            }
        }
        self.audio_clip_inspector_edits
            .remove(&(clip_id, field.inspector_field()));
        self.audio_clip_transpose_debounce.remove(&clip_id);
        action.mark_dirty = true;
        action
    }

    pub(super) fn preview_audio_clip_rotary_value(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipRotaryField,
        value: f32,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if field != AudioClipRotaryField::Transpose {
            return action;
        }
        let semitones = value
            .round()
            .clamp(f32::from(ClipTranspose::MIN), f32::from(ClipTranspose::MAX))
            as i8;
        self.audio_clip_inspector_edits.insert(
            (clip_id, AudioClipInspectorField::Transpose),
            semitones.to_string(),
        );
        let revision = self
            .audio_clip_transpose_debounce
            .entry(clip_id)
            .and_modify(|revision| *revision = revision.wrapping_add(1))
            .or_insert(1);
        action.transpose_debounce = Some((track_id, clip_id, semitones, *revision));
        action
    }
}
