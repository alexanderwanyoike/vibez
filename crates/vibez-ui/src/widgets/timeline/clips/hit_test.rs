//! Clip hit testing and marquee message construction.

use super::*;

/// The Clip under the pointer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClipHit {
    pub clip_id: ClipId,
    pub is_note_clip: bool,
    pub near_right: bool,
    pub position_beats: f64,
    pub duration_beats: f64,
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

    /// Find a clip at the given pixel position.
    pub(crate) fn hit_test(&self, pos_x: f32) -> Option<ClipHit> {
        let geometry = self.geometry();
        let spb = self.spb();

        for clip in &self.clips {
            let clip_start_beat = clip.position as f64 / spb;
            let clip_dur_beats = clip.duration as f64 / spb;
            let clip_x = self.beat_to_x(clip_start_beat);
            let clip_w = geometry.width_for_beats(clip_dur_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                return Some(ClipHit {
                    clip_id: clip.clip_id,
                    is_note_clip: false,
                    near_right: pos_x > clip_x + clip_w - RESIZE_EDGE_PX,
                    position_beats: clip_start_beat,
                    duration_beats: clip_dur_beats,
                });
            }
        }

        for note_clip in &self.note_clips {
            let clip_x = self.beat_to_x(note_clip.position_beats);
            let clip_w = geometry.width_for_beats(note_clip.duration_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                return Some(ClipHit {
                    clip_id: note_clip.clip_id,
                    is_note_clip: true,
                    near_right: pos_x > clip_x + clip_w - RESIZE_EDGE_PX,
                    position_beats: note_clip.position_beats,
                    duration_beats: note_clip.duration_beats,
                });
            }
        }

        None
    }
}
