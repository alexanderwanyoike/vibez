//! Pointer travel on a held Clip, turned into its move or edge edit.

use iced::Point;

use super::*;
use crate::state::{ClipDragKind, ClipDragStep};
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
        let snapped =
            self.snap_targets
                .moved_span(origin.start_beats, origin.length_beats, travel, snap);

        // Resolve against real row geometry when the surface supplied it;
        // stepping by a fixed row height mis-targets as soon as a track
        // above has its automation lanes expanded.
        let target_track = self
            .resolve_lane_at(self.row_top() + local.y)
            .filter(|_| (local.y - *start_y).abs() > CROSS_TRACK_MIN_PX)
            .and_then(|lane| self.track_ids.get(lane).copied())
            .unwrap_or(self.track_id);
        let step = ClipDragStep {
            gesture: *undo_gesture,
            kind: ClipDragKind::Move,
            anchor: *clip_id,
            origin_track: self.track_id,
            origin: *origin,
            guide: snapped.target,
            pointer: Point::new(local.x, self.row_top() + local.y),
        };
        Some(drag_step_message(
            step,
            ArrangementMsg::MoveSelectedClips {
                anchor: *clip_id,
                new_position_beats: snapped.beat.max(0.0),
                target_track,
            },
        ))
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
        let track_id = self.track_id;
        let anchor = *anchor;
        let (snapped, edit) = match edge {
            ClipEdge::Start => {
                let start = self.snap_targets.span_start(pointer, fixed_beat, snap);
                let edit = ArrangementMsg::TrimSelectedClipsStart {
                    anchor,
                    new_start_beats: start.beat,
                };
                (start, edit)
            }
            ClipEdge::End => {
                let end = self.snap_targets.span_end(fixed_beat, pointer, snap);
                let edit = ArrangementMsg::ResizeSelectedClips {
                    anchor,
                    new_duration_beats: end.beat - fixed_beat,
                };
                (end, edit)
            }
        };
        let step = ClipDragStep {
            gesture: *undo_gesture,
            kind: match edge {
                ClipEdge::Start => ClipDragKind::Start,
                ClipEdge::End => ClipDragKind::End,
            },
            anchor: anchor.clip_id(),
            origin_track: track_id,
            origin: *origin,
            guide: snapped.target,
            pointer: Point::new(local.x, self.row_top() + local.y),
        };
        Some(drag_step_message(step, edit))
    }
}

fn drag_step_message(step: ClipDragStep, edit: ArrangementMsg) -> Message {
    let gesture = step.gesture;
    Message::Arrangement(ArrangementMsg::DragClips {
        step,
        edit: Box::new(edit),
    })
    .in_undo_gesture(gesture)
}
