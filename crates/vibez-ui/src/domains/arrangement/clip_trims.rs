//! Left-edge trims: a Clip's start moves while its content keeps its
//! Arrangement time.

use vibez_core::track::ClipPlaybackDirection;

use super::fragment_geometry::audio_fragment;
use super::{ArrangementAction, ArrangementCtx, EngineHandle};
use crate::state::SHORTEST_LENGTH_BEATS;
use crate::state::{ArrangementSelection, TimelineEditorState, UiClip};

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
        let Some(anchor_start) = self.clip_span(anchor, spb).map(|span| span.start_beats) else {
            return ArrangementAction::default();
        };
        let delta = new_start_beats - anchor_start;
        let targets: Vec<_> = if self.selected_clips.contains(&anchor) {
            self.selected_clips.iter().copied().collect()
        } else {
            vec![anchor]
        };
        let shortest_frames = (SHORTEST_LENGTH_BEATS * spb).round() as u64;
        let mut changed = false;
        for target in targets {
            if let ArrangementSelection::AudioClip { track_id, clip_id } = target {
                let trimmed = self
                    .find_content(track_id)
                    .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
                    .map(|clip| {
                        audio_start_trimmed(clip, (delta * spb).round() as i64, shortest_frames)
                    });
                if let Some(trimmed) = trimmed.flatten() {
                    changed = true;
                    self.replace_audio_clip(engine, track_id, clip_id, vec![trimmed]);
                }
            }
        }
        if !changed {
            return ArrangementAction::default();
        }
        self.drag_resize_active = true;
        ArrangementAction {
            mark_dirty: true,
            ..Default::default()
        }
    }
}

/// `clip` with its start `delta` frames later (earlier when negative), or
/// `None` when it cannot move that way.
fn audio_start_trimmed(clip: &UiClip, delta: i64, shortest: u64) -> Option<UiClip> {
    if delta > 0 {
        let cut = (delta as u64).min(clip.duration.saturating_sub(shortest));
        if cut == 0 {
            return None;
        }
        let mut trimmed = audio_fragment(clip, clip.name.clone(), cut, clip.duration - cut);
        trimmed.id = clip.id;
        return Some(trimmed);
    }
    // Extending a looped Clip earlier repeats its loop rather than revealing
    // the source before it, which is a different edit.
    if clip.loop_enabled {
        return None;
    }
    let wanted = delta.unsigned_abs().min(clip.position);
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
