//! Clip hit testing and marquee message construction.

use super::*;
use crate::widgets::timeline::clip_drag::ClipEdge;
use crate::widgets::timeline::clip_frame::clip_box;

/// The Clip under the pointer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClipHit {
    pub clip_id: ClipId,
    pub is_note_clip: bool,
    /// The edge whose resize zone the pointer is in.
    pub edge: Option<ClipEdge>,
    pub position_beats: f64,
    pub duration_beats: f64,
}

/// The resize zone under `pos_x` of a Clip drawn from `clip_x`, `clip_w`
/// wide. Each edge's zone runs the Clip's full height and shrinks with the
/// Clip's width, as in Ableton, so zooming out never turns a move into a
/// resize.
fn edge_at(pos_x: f32, clip_x: f32, clip_w: f32) -> Option<ClipEdge> {
    if clip_w < RESIZABLE_CLIP_MIN_PX {
        return None;
    }
    let zone = RESIZE_EDGE_PX.min(clip_w * RESIZE_EDGE_MAX_FRACTION);
    let from_start = pos_x - clip_x;
    let from_end = clip_x + clip_w - pos_x;
    if from_end < zone {
        Some(ClipEdge::End)
    } else if from_start < zone {
        Some(ClipEdge::Start)
    } else {
        None
    }
}

impl TrackClipCanvas {
    /// Samples per beat for Audio Clip geometry.
    pub(crate) fn spb(&self) -> f64 {
        if self.bpm > 0.0 {
            self.sample_rate as f64 * 60.0 / self.bpm
        } else {
            1.0
        }
    }

    /// Build the marquee message for a drag from `anchor` to the pointer,
    /// both already in column coordinates.
    pub(super) fn marquee_message(
        &self,
        anchor_beat: f64,
        anchor_column_y: f32,
        current_x: f32,
        current_column_y: f32,
        additive: bool,
    ) -> Message {
        let current_beat = self.snapped_beat(self.geometry().x_to_beat(current_x));
        let span = marquee::resolve(
            anchor_beat,
            current_beat,
            anchor_column_y,
            current_column_y,
            &self.row_spans,
        );
        Message::Arrangement(ArrangementMsg::MarqueeSelect {
            anchor_track: self.track_id,
            start_beats: span.start_beats,
            end_beats: span.end_beats,
            top_y: span.top_y,
            bottom_y: span.bottom_y,
            track_ids: span.track_ids,
            additive,
        })
    }

    /// The Clip under `pos_x`, searched in reverse draw order so the Clip
    /// drawn on top wins where Clips overlap.
    pub(crate) fn hit_test(&self, pos_x: f32) -> Option<ClipHit> {
        let geometry = self.geometry();
        let spb = self.spb();
        let audio_clips = self.clips.iter().rev().map(|clip| ClipHit {
            clip_id: clip.clip_id,
            is_note_clip: false,
            edge: None,
            position_beats: clip.position as f64 / spb,
            duration_beats: clip.duration as f64 / spb,
        });
        let note_clips = self.note_clips.iter().rev().map(|clip| ClipHit {
            clip_id: clip.clip_id,
            is_note_clip: true,
            edge: None,
            position_beats: clip.position_beats,
            duration_beats: clip.duration_beats,
        });

        note_clips.chain(audio_clips).find_map(|hit| {
            let body = clip_box(
                self.beat_to_x(hit.position_beats),
                CLIP_Y,
                geometry.width_for_beats(hit.duration_beats),
                0.0,
            );
            (pos_x >= body.x && pos_x <= body.x + body.width).then(|| ClipHit {
                edge: edge_at(pos_x, body.x, body.width),
                ..hit
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_clip_resizes_from_a_full_edge_zone_on_both_sides() {
        assert_eq!(edge_at(107.0, 100.0, 200.0), Some(ClipEdge::Start));
        assert_eq!(edge_at(293.0, 100.0, 200.0), Some(ClipEdge::End));
        assert_eq!(edge_at(109.0, 100.0, 200.0), None);
        assert_eq!(edge_at(291.0, 100.0, 200.0), None);
    }

    #[test]
    fn a_short_clip_keeps_a_move_zone_in_its_middle() {
        assert_eq!(edge_at(103.0, 100.0, 16.0), Some(ClipEdge::Start));
        assert_eq!(edge_at(113.0, 100.0, 16.0), Some(ClipEdge::End));
        assert_eq!(edge_at(105.0, 100.0, 16.0), None);
        assert_eq!(edge_at(108.0, 100.0, 16.0), None);
        assert_eq!(edge_at(111.0, 100.0, 16.0), None);
    }

    #[test]
    fn a_tiny_clip_only_moves() {
        for x in [100.0, 100.5, 102.0, 103.5, 104.0] {
            assert_eq!(edge_at(x, 100.0, 4.0), None, "at {x}");
        }
    }
}
