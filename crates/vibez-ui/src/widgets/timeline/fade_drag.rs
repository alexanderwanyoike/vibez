//! Audio Clip fade-handle hit testing and reciprocal drag geometry.

use iced::Point;

use crate::state::{AudioClipFadeEdge, UndoGestureId};
use vibez_core::id::ClipId;
use vibez_core::track::FadeCurve;

use super::clips::TrackClipCanvas;
use super::{TimelineClip, CLIP_TITLE_HEIGHT, CLIP_Y, FADE_HANDLE_HIT_RADIUS, FADE_HANDLE_Y};

#[derive(Debug, Clone)]
pub struct FadeClipDrag {
    pub undo_gesture: UndoGestureId,
    pub clip_id: ClipId,
    pub edge: AudioClipFadeEdge,
    clip_x: f32,
    clip_width: f32,
    duration_frames: u64,
    handle: Point,
}

#[derive(Debug, Clone)]
pub struct FadeCurveDrag {
    pub undo_gesture: UndoGestureId,
    pub clip_id: ClipId,
    pub edge: AudioClipFadeEdge,
    top: f32,
    bottom: f32,
    handle: Point,
}

pub(super) enum FadeControlDrag {
    Length(FadeClipDrag),
    Curve(FadeCurveDrag),
}

#[derive(Debug, Clone)]
pub struct CrossfadeCurveDrag {
    pub undo_gesture: UndoGestureId,
    pub outgoing_id: ClipId,
    pub incoming_id: ClipId,
    top: f32,
    bottom: f32,
}

impl CrossfadeCurveDrag {
    fn new(outgoing_id: ClipId, incoming_id: ClipId, canvas_height: f32) -> Self {
        Self {
            undo_gesture: UndoGestureId::new(),
            outgoing_id,
            incoming_id,
            top: CLIP_Y + CLIP_TITLE_HEIGHT + 2.0,
            bottom: canvas_height - CLIP_Y - 2.0,
        }
    }

    pub(super) fn curve_at_y(&self, y: f32) -> FadeCurve {
        let height = (self.bottom - self.top).max(1.0);
        let min_gain = FadeCurve::new(-100).crossfade_gains(0.5).1;
        let max_gain = FadeCurve::new(100).crossfade_gains(0.5).1;
        let gain = ((self.bottom - y) / height).clamp(min_gain, max_gain);
        let warped = gain.asin() / std::f32::consts::FRAC_PI_2;
        let exponent = warped.ln() / 0.5_f32.ln();
        FadeCurve::new((-50.0 * exponent.log2()).round() as i16)
    }
}

impl FadeCurveDrag {
    fn new(clip_id: ClipId, edge: AudioClipFadeEdge, canvas_height: f32, handle: Point) -> Self {
        Self {
            undo_gesture: UndoGestureId::new(),
            clip_id,
            edge,
            top: CLIP_Y + CLIP_TITLE_HEIGHT + 2.0,
            bottom: canvas_height - CLIP_Y - 2.0,
            handle,
        }
    }

    fn distance_squared(&self, point: Point) -> f32 {
        (point.x - self.handle.x).powi(2) + (point.y - self.handle.y).powi(2)
    }

    pub(super) fn curve_at_y(&self, y: f32) -> FadeCurve {
        let height = (self.bottom - self.top).max(1.0);
        let gain = ((self.bottom - y) / height).clamp(
            FadeCurve::new(-100).gain(0.5),
            FadeCurve::new(100).gain(0.5),
        );
        let exponent = gain.ln() / 0.5_f32.ln();
        FadeCurve::new((-50.0 * exponent.log2()).round() as i16)
    }
}

impl FadeClipDrag {
    fn new(
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
        clip_x: f32,
        clip_width: f32,
        duration_frames: u64,
        handle: Point,
    ) -> Self {
        Self {
            undo_gesture: UndoGestureId::new(),
            clip_id,
            edge,
            clip_x,
            clip_width,
            duration_frames,
            handle,
        }
    }

    fn distance_squared(&self, point: Point) -> f32 {
        (point.x - self.handle.x).powi(2) + (point.y - self.handle.y).powi(2)
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

pub(super) fn fade_curve_handle(
    clip_x: f32,
    clip_width: f32,
    canvas_height: f32,
    clip: &TimelineClip,
    edge: AudioClipFadeEdge,
) -> Option<Point> {
    let (fade_in_x, fade_out_x) = fade_handle_xs(clip_x, clip_width, clip);
    let (start_x, end_x, curve, linked) = match edge {
        AudioClipFadeEdge::In => (
            clip_x,
            fade_in_x,
            clip.fade_in_curve,
            clip.crossfade_in_from.is_some(),
        ),
        AudioClipFadeEdge::Out => (
            fade_out_x,
            clip_x + clip_width,
            clip.fade_out_curve,
            clip.crossfade_out_to.is_some(),
        ),
    };
    if linked || (end_x - start_x).abs() <= f32::EPSILON {
        return None;
    }
    let top = CLIP_Y + CLIP_TITLE_HEIGHT + 2.0;
    let bottom = canvas_height - CLIP_Y - 2.0;
    Some(Point::new(
        (start_x + end_x) / 2.0,
        bottom - (bottom - top) * curve.gain(0.5),
    ))
}

impl TrackClipCanvas {
    pub(super) fn crossfade_curve_handles(
        &self,
        canvas_height: f32,
    ) -> Vec<(ClipId, ClipId, Point)> {
        let top = CLIP_Y + CLIP_TITLE_HEIGHT + 2.0;
        let bottom = canvas_height - CLIP_Y - 2.0;
        let spb = self.spb();
        self.clips
            .iter()
            .filter_map(|outgoing| {
                let incoming_id = outgoing.crossfade_out_to?;
                let incoming = self.clips.iter().find(|clip| {
                    clip.clip_id == incoming_id && clip.crossfade_in_from == Some(outgoing.clip_id)
                })?;
                let overlap_start = incoming.position.max(outgoing.position);
                let overlap_end = incoming
                    .position
                    .saturating_add(incoming.duration)
                    .min(outgoing.position.saturating_add(outgoing.duration));
                if overlap_end <= overlap_start {
                    return None;
                }
                let x = (self.beat_to_x(overlap_start as f64 / spb)
                    + self.beat_to_x(overlap_end as f64 / spb))
                    / 2.0;
                let incoming_gain = outgoing.fade_out_curve.crossfade_gains(0.5).1;
                let y = bottom - (bottom - top) * incoming_gain;
                Some((outgoing.clip_id, incoming_id, Point::new(x, y)))
            })
            .collect()
    }

    pub(super) fn crossfade_curve_hit(
        &self,
        pos: Point,
        canvas_height: f32,
    ) -> Option<CrossfadeCurveDrag> {
        self.crossfade_curve_handles(canvas_height)
            .into_iter()
            .find(|(outgoing_id, incoming_id, handle)| {
                (self.selected_clips.contains(outgoing_id)
                    || self.selected_clips.contains(incoming_id))
                    && (pos.x - handle.x).abs() <= FADE_HANDLE_HIT_RADIUS
                    && (pos.y - handle.y).abs() <= FADE_HANDLE_HIT_RADIUS
            })
            .map(|(outgoing_id, incoming_id, _)| {
                CrossfadeCurveDrag::new(outgoing_id, incoming_id, canvas_height)
            })
    }

    pub(super) fn fade_curve_hit(&self, pos: Point, canvas_height: f32) -> Option<FadeCurveDrag> {
        let spb = self.spb();
        for clip in &self.clips {
            if !self.selected_clips.contains(&clip.clip_id) {
                continue;
            }
            let clip_x = self.beat_to_x(clip.position as f64 / spb);
            let clip_width = self.geometry().width_for_beats(clip.duration as f64 / spb);
            for edge in [AudioClipFadeEdge::In, AudioClipFadeEdge::Out] {
                let Some(handle) = fade_curve_handle(clip_x, clip_width, canvas_height, clip, edge)
                else {
                    continue;
                };
                if (pos.x - handle.x).abs() <= FADE_HANDLE_HIT_RADIUS
                    && (pos.y - handle.y).abs() <= FADE_HANDLE_HIT_RADIUS
                {
                    return Some(FadeCurveDrag::new(
                        clip.clip_id,
                        edge,
                        canvas_height,
                        handle,
                    ));
                }
            }
        }
        None
    }

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
                let handle_x = match edge {
                    AudioClipFadeEdge::In => fade_in_x,
                    AudioClipFadeEdge::Out => fade_out_x,
                };
                return Some(FadeClipDrag::new(
                    clip.clip_id,
                    edge,
                    clip_x,
                    clip_width,
                    clip.duration,
                    Point::new(handle_x, FADE_HANDLE_Y),
                ));
            }
        }
        None
    }

    /// Resolve overlapping fade controls by their drawn distance. The
    /// fade-length handle wins an exact tie so a short steep fade can always
    /// be made longer again.
    pub(super) fn fade_control_hit(
        &self,
        pos: Point,
        canvas_height: f32,
    ) -> Option<FadeControlDrag> {
        match (
            self.fade_handle_hit(pos),
            self.fade_curve_hit(pos, canvas_height),
        ) {
            (Some(length), Some(curve)) => {
                if length.distance_squared(pos) <= curve.distance_squared(pos) {
                    Some(FadeControlDrag::Length(length))
                } else {
                    Some(FadeControlDrag::Curve(curve))
                }
            }
            (Some(length), None) => Some(FadeControlDrag::Length(length)),
            (None, Some(curve)) => Some(FadeControlDrag::Curve(curve)),
            (None, None) => None,
        }
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
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
            crossfade_in_from: None,
            crossfade_out_to: None,
            warp_stale: false,
        };
        let (fade_in_x, fade_out_x) = fade_handle_xs(37.0, 481.0, &clip);

        let fade_in = FadeClipDrag::new(
            clip.clip_id,
            AudioClipFadeEdge::In,
            37.0,
            481.0,
            clip.duration,
            Point::new(fade_in_x, FADE_HANDLE_Y),
        );
        let fade_out = FadeClipDrag::new(
            clip.clip_id,
            AudioClipFadeEdge::Out,
            37.0,
            481.0,
            clip.duration,
            Point::new(fade_out_x, FADE_HANDLE_Y),
        );

        assert_eq!(fade_in.frames_at_x(fade_in_x), clip.fade_in_frames);
        assert_eq!(fade_out.frames_at_x(fade_out_x), clip.fade_out_frames);
    }
}
