//! Clip hit testing and marquee message construction.

use super::*;

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
    ///
    /// Returns `(clip_id, is_note_clip, near_right_edge, position_beats,
    /// duration_beats)`.
    pub(crate) fn hit_test(&self, pos_x: f32) -> Option<(ClipId, bool, bool, f64, f64)> {
        let geometry = self.geometry();
        let spb = self.spb();

        for clip in &self.clips {
            let clip_start_beat = clip.position as f64 / spb;
            let clip_dur_beats = clip.duration as f64 / spb;
            let clip_x = self.beat_to_x(clip_start_beat);
            let clip_w = geometry.width_for_beats(clip_dur_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    clip.clip_id,
                    false,
                    near_right,
                    clip_start_beat,
                    clip_dur_beats,
                ));
            }
        }

        for note_clip in &self.note_clips {
            let clip_x = self.beat_to_x(note_clip.position_beats);
            let clip_w = geometry.width_for_beats(note_clip.duration_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    note_clip.clip_id,
                    true,
                    near_right,
                    note_clip.position_beats,
                    note_clip.duration_beats,
                ));
            }
        }

        None
    }
}
