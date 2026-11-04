//! Pointer travel on a held Clip, turned into its move or edge edit.

use iced::Point;

use super::*;
use crate::widgets::timeline::clip_drag::ClipEdge;

impl TrackClipCanvas {
    /// The edit for a Clip held by its title and dragged to `local`, or
    /// `None` while the pointer is still inside the click slop.
    pub(super) fn move_step(
        &self,
        state: &mut ClipInteractionState,
        local: Point,
    ) -> Option<Message> {
        let snap = self.snap_context(state.alt_held);
        let Some(ClipDragAction::MoveClip {
            undo_gesture,
            clip_id,
            start_local_x,
            start_scroll_beats,
            origin,
            start_y,
            phase,
            ..
        }) = &mut state.drag
        else {
            return None;
        };
        if !phase.follow(Point::new(*start_local_x, *start_y), local) {
            return None;
        }
        let travel = crate::timeline_geometry::compensated_drag_beat(
            origin.start_beats,
            local.x - *start_local_x,
            self.pixels_per_beat(),
            *start_scroll_beats,
            self.scroll_offset_beats,
        ) - origin.start_beats;
        let new_pos = self
            .snap_targets
            .moved_span(origin.start_beats, origin.length_beats, travel, snap)
            .beat
            .max(0.0);

        // Resolve against real row geometry when the surface supplied it;
        // stepping by a fixed row height mis-targets as soon as a track
        // above has its automation lanes expanded.
        let target_track = self
            .resolve_lane_at(self.row_top() + local.y)
            .filter(|_| (local.y - *start_y).abs() > CROSS_TRACK_MIN_PX)
            .and_then(|lane| self.track_ids.get(lane).copied())
            .unwrap_or(self.track_id);
        Some(
            Message::Arrangement(ArrangementMsg::MoveSelectedClips {
                anchor: *clip_id,
                new_position_beats: new_pos,
                target_track,
            })
            .in_undo_gesture(*undo_gesture),
        )
    }

    /// The edit for a Clip held by an edge and dragged to `local`, or
    /// `None` while the pointer is still inside the click slop.
    pub(super) fn edge_step(
        &self,
        state: &mut ClipInteractionState,
        local: Point,
    ) -> Option<Message> {
        let snap = self.snap_context(state.alt_held);
        let Some(ClipDragAction::ResizeClip {
            undo_gesture,
            anchor,
            edge,
            origin,
            start_local_x,
            phase,
        }) = &mut state.drag
        else {
            return None;
        };
        if !phase.follow_x(*start_local_x, local.x) {
            return None;
        }
        let fixed_beat = edge.fixed_beat(*origin);
        let pointer = self.x_to_beat(local.x);
        let anchor = *anchor;
        let edit = match edge {
            ClipEdge::Start => ArrangementMsg::TrimSelectedClipsStart {
                anchor,
                new_start_beats: self.snap_targets.span_start(pointer, fixed_beat, snap).beat,
            },
            ClipEdge::End => ArrangementMsg::ResizeSelectedClips {
                anchor,
                new_duration_beats: self.snap_targets.span_end(fixed_beat, pointer, snap).beat
                    - fixed_beat,
            },
        };
        Some(Message::Arrangement(edit).in_undo_gesture(*undo_gesture))
    }
}
