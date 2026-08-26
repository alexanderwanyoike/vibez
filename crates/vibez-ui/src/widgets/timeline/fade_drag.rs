//! Audio Clip fade-handle hit testing and reciprocal drag geometry.

use iced::Point;

use crate::state::{AudioClipFadeEdge, UndoGestureId};
use vibez_core::id::ClipId;

use super::clips::TrackClipCanvas;
use super::{TimelineClip, FADE_HANDLE_HIT_RADIUS, FADE_HANDLE_Y};

#[derive(Debug, Clone)]
pub struct FadeClipDrag {
    pub undo_gesture: UndoGestureId,
    pub clip_id: ClipId,
    pub edge: AudioClipFadeEdge,
    clip_x: f32,
    clip_width: f32,
    duration_frames: u64,
}

impl FadeClipDrag {
    fn new(
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
        clip_x: f32,
        clip_width: f32,
        duration_frames: u64,
    ) -> Self {
        Self {
            undo_gesture: UndoGestureId::new(),
            clip_id,
            edge,
            clip_x,
            clip_width,
            duration_frames,
        }
    }

    /// Use the exact inverse of [`fade_handle_xs`] so a handle drawn at one
    /// frame cannot jitter to an adjacent frame when the pointer has not moved.
    pub(super) fn frames_at_x(&self, x: f32) -> u64 {
        if self.clip_width <= 0.0 || self.duration_frames == 0 {
            return 0;
        }
        let progress = ((x - self.clip_x) / self.clip_width).clamp(0.0, 1.0);
        let progress = match self.edge {
            AudioClipFadeEdge::In => progress,
            AudioClipFadeEdge::Out => 1.0 - progress,
        };
        (progress as f64 * self.duration_frames as f64).round() as u64
    }
}

pub(super) fn fade_handle_xs(clip_x: f32, clip_width: f32, clip: &TimelineClip) -> (f32, f32) {
    if clip.duration == 0 {
        return (clip_x, clip_x + clip_width);
    }
    let pixels_per_frame = clip_width / clip.duration as f32;
    (
        clip_x + clip.fade_in_frames as f32 * pixels_per_frame,
        clip_x + clip_width - clip.fade_out_frames as f32 * pixels_per_frame,
    )
}

impl TrackClipCanvas {
    pub(super) fn fade_handle_hit(&self, pos: Point) -> Option<FadeClipDrag> {
        if (pos.y - FADE_HANDLE_Y).abs() > FADE_HANDLE_HIT_RADIUS {
            return None;
        }
        let spb = self.spb();
        for clip in &self.clips {
            if !self.selected_clips.contains(&clip.clip_id) {
                continue;
            }
            let start_beat = clip.position as f64 / spb;
            let clip_x = self.beat_to_x(start_beat);
            let clip_width = self.geometry().width_for_beats(clip.duration as f64 / spb);
            if pos.x < clip_x || pos.x > clip_x + clip_width {
                continue;
            }
            let (fade_in_x, fade_out_x) = fade_handle_xs(clip_x, clip_width, clip);
            let in_distance = (pos.x - fade_in_x).abs();
            let out_distance = (pos.x - fade_out_x).abs();
            let edge = if in_distance <= FADE_HANDLE_HIT_RADIUS
                && (in_distance < out_distance
                    || (in_distance == out_distance && pos.x <= clip_x + clip_width / 2.0))
            {
                Some(AudioClipFadeEdge::In)
            } else if out_distance <= FADE_HANDLE_HIT_RADIUS {
                Some(AudioClipFadeEdge::Out)
            } else {
                None
            };
            if let Some(edge) = edge {
                return Some(FadeClipDrag::new(
                    clip.clip_id,
                    edge,
                    clip_x,
                    clip_width,
                    clip.duration,
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_math_is_reciprocal_with_drawn_handle_positions() {
        let clip = TimelineClip {
            clip_id: ClipId::new(),
            position: 0,
            duration: 48_000,
            name: String::new(),
            peaks: Default::default(),
            peak_span_frames: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 48_000,
            fade_in_frames: 12_345,
            fade_out_frames: 6_789,
            warp_stale: false,
        };
        let (fade_in_x, fade_out_x) = fade_handle_xs(37.0, 481.0, &clip);

        let fade_in = FadeClipDrag::new(
            clip.clip_id,
            AudioClipFadeEdge::In,
            37.0,
            481.0,
            clip.duration,
        );
        let fade_out = FadeClipDrag::new(
            clip.clip_id,
            AudioClipFadeEdge::Out,
            37.0,
            481.0,
            clip.duration,
        );

        assert_eq!(fade_in.frames_at_x(fade_in_x), clip.fade_in_frames);
        assert_eq!(fade_out.frames_at_x(fade_out_x), clip.fade_out_frames);
    }
}
