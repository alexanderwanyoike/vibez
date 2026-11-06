//! Left-edge trims: a Clip's start moves while its content keeps its
//! Arrangement time.

use vibez_core::track::ClipPlaybackDirection;

use super::fragment_geometry::audio_fragment;
use super::{ArrangementAction, ArrangementCtx, EngineHandle};
use crate::state::{
    ArrangementSelection, EditDrag, TimelineEditorState, UiClip, UiNoteClip, SHORTEST_LENGTH_BEATS,
};

impl TimelineEditorState {
    pub(super) fn op_trim_selected_clips_start(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        anchor: ArrangementSelection,
        new_start_beats: f64,
    ) -> ArrangementAction {
        let spb = ctx.samples_per_beat;
        if spb <= 0.0 {
            return ArrangementAction::default();
        }
        let origin = match &self.edit_drag {
            EditDrag::Trimming(origin) => origin.clone(),
            _ => self.timeline.clone(),
        };
        let Some(anchor_start) = origin.clip_span(anchor, spb).map(|span| span.start_beats) else {
            return ArrangementAction::default();
        };
        let delta = new_start_beats - anchor_start;
        let targets = self.gesture_targets(anchor);
        let shortest_frames = (SHORTEST_LENGTH_BEATS * spb).round() as u64;
        let mut changed = false;
        for target in targets {
            let Some(lane) = origin.get(target.track_id()) else {
                continue;
            };
            match target {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    let Some(start) = lane.clips.iter().find(|c| c.id == clip_id) else {
                        continue;
                    };
                    let trimmed =
                        audio_start_trimmed(start, (delta * spb).round() as i64, shortest_frames)
                            .unwrap_or_else(|| start.clone());
                    let live = self
                        .find_content(track_id)
                        .and_then(|t| t.clips.iter().find(|c| c.id == clip_id));
                    if live != Some(&trimmed) {
                        changed = true;
                        self.replace_audio_clip(engine, track_id, clip_id, vec![trimmed]);
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } => {
                    let Some(start) = lane.note_clips.iter().find(|c| c.id == clip_id) else {
                        continue;
                    };
                    let trimmed = note_start_trimmed(start, delta).unwrap_or_else(|| start.clone());
                    let live = self
                        .find_content(track_id)
                        .and_then(|t| t.note_clips.iter().find(|c| c.id == clip_id));
                    if live != Some(&trimmed) {
                        changed = true;
                        self.replace_note_clip(engine, track_id, clip_id, vec![trimmed]);
                    }
                }
            }
        }
        if !changed {
            return ArrangementAction::default();
        }
        self.edit_drag = EditDrag::Trimming(origin);
        ArrangementAction {
            mark_dirty: true,
            ..Default::default()
        }
    }
}

/// `clip` with its start `delta` frames later (earlier when negative), or
/// `None` when it cannot move that way.
pub(super) fn audio_start_trimmed(clip: &UiClip, delta: i64, shortest: u64) -> Option<UiClip> {
    if delta > 0 {
        let cut = (delta as u64).min(clip.duration.saturating_sub(shortest));
        if cut == 0 {
            return None;
        }
        let mut trimmed = audio_fragment(clip, clip.name.clone(), cut, clip.duration - cut);
        trimmed.id = clip.id;
        return Some(trimmed);
    }
    let wanted = delta.unsigned_abs().min(clip.position);
    if clip.loop_enabled {
        return audio_loop_extended(clip, wanted);
    }
    let mut trimmed = clip.clone();
    match clip.playback_direction {
        ClipPlaybackDirection::Forward => {
            let within = wanted.min(clip.start_marker.saturating_sub(clip.source_offset));
            trimmed.start_marker -= within;
            let (gained, source_offset, warp_markers) = clip
                .warp_markers
                .extended_start(wanted - within, clip.source_offset);
            if gained > 0 {
                trimmed.source_offset = source_offset;
                trimmed.start_marker = source_offset;
                trimmed.warp_markers = warp_markers;
            }
            trimmed.duration += within + gained;
        }
        // Reversed, the left edge plays the end of the source window, so
        // reaching earlier in time lengthens the window at its end.
        ClipPlaybackDirection::Reverse => trimmed.resize(clip.duration + wanted),
    }
    let gained = trimmed
        .duration
        .checked_sub(clip.duration)
        .filter(|g| *g > 0)?;
    trimmed.position -= gained;
    Some(trimmed)
}

/// `clip` reaching `wanted` frames earlier by repeating its loop there, or
/// `None` when nothing repeats before its start.
fn audio_loop_extended(clip: &UiClip, wanted: u64) -> Option<UiClip> {
    let start_marker = match clip.playback_direction {
        ClipPlaybackDirection::Forward => clip.timeline().start_before(wanted)?,
        // Reversed, the left edge plays the far end of the Clip's run
        // through its loop, which a longer run simply continues.
        ClipPlaybackDirection::Reverse => {
            clip.timeline().is_looping().then_some(clip.start_marker)?
        }
    };
    if wanted == 0 {
        return None;
    }
    let mut extended = clip.clone();
    extended.start_marker = start_marker;
    extended.position -= wanted;
    extended.duration += wanted;
    Some(extended)
}

/// `clip` with its start `delta` beats later (earlier when negative), or
/// `None` when it cannot move that way. Its notes stay where they are in
/// the content; the start marker moves over them.
pub(super) fn note_start_trimmed(clip: &UiNoteClip, delta: f64) -> Option<UiNoteClip> {
    let timeline = clip.timeline();
    let delta = delta
        .max(-clip.position_beats)
        .min((clip.duration_beats - SHORTEST_LENGTH_BEATS).max(0.0));
    let repeated_start = (delta < 0.0)
        .then(|| timeline.start_before(-delta))
        .flatten();
    let (delta, start_marker) = match repeated_start {
        Some(start_marker) => (delta, start_marker),
        None => {
            // A looped Clip opening on an intro repeats nothing before it.
            let earliest = if clip.loop_enabled {
                clip.loop_start_beats.min(clip.start_marker_beats)
            } else {
                0.0
            };
            let delta = delta.max(earliest - clip.start_marker_beats);
            (delta, timeline.source_at(delta))
        }
    };
    if delta == 0.0 {
        return None;
    }
    let mut trimmed = clip.clone();
    trimmed.start_marker_beats = start_marker;
    trimmed.position_beats += delta;
    trimmed.duration_beats -= delta;
    Some(trimmed)
}
