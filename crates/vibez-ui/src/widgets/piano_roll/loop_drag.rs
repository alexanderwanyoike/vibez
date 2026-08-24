//! Loop-brace interaction for the piano roll.

use iced::{Point, Rectangle};

use crate::domains::piano_roll::PianoRollMsg;
use crate::message::Message;
use crate::widgets::clip_loop_markers::{self, LoopDrag, LoopMarker, MARKER_RAIL_HEIGHT};

use super::{DragAction, PianoRollWidget, KEY_WIDTH};

impl PianoRollWidget {
    pub(super) fn hit_test_loop_marker(
        &self,
        position: Point,
        bounds: &Rectangle,
    ) -> Option<LoopMarker> {
        let clip = self.clip.as_ref()?;
        if !clip.loop_enabled || clip.loop_end_beats <= clip.loop_start_beats {
            return None;
        }
        clip_loop_markers::hit_test(
            self.beat_to_x(clip.loop_start_beats, bounds),
            self.beat_to_x(clip.loop_end_beats, bounds),
            position,
            MARKER_RAIL_HEIGHT,
        )
    }

    pub(super) fn begin_loop_drag(
        &self,
        position: Point,
        bounds: &Rectangle,
    ) -> Option<DragAction> {
        let marker = self.hit_test_loop_marker(position, bounds)?;
        let clip = self.clip.as_ref()?;
        Some(DragAction::Loop {
            clip_id: clip.clip_id,
            drag: LoopDrag::begin(marker, clip.loop_start_beats, clip.loop_end_beats),
        })
    }

    pub(super) fn loop_drag_message(
        &self,
        action: &DragAction,
        position: Point,
        bounds: &Rectangle,
    ) -> Option<Message> {
        let DragAction::Loop { clip_id, drag } = action else {
            return None;
        };
        let clip = self.clip.as_ref()?;
        let min_length = if self.grid.snap_enabled {
            self.effective_grid(bounds).beat_size()
        } else {
            0.01
        };
        let pointer = self.snapped_beat(
            self.x_to_beat(position.x.clamp(KEY_WIDTH, bounds.width), bounds),
            bounds,
        );
        let (loop_start_beats, loop_end_beats) =
            drag.resolve(pointer, min_length.min(self.total_beats), self.total_beats);
        if (loop_start_beats, loop_end_beats) == (clip.loop_start_beats, clip.loop_end_beats) {
            return None;
        }

        Some(
            Message::PianoRoll(PianoRollMsg::SetNoteClipLoopRegion {
                track_id: self.track_id,
                clip_id: *clip_id,
                loop_start_beats,
                loop_end_beats,
            })
            .in_undo_gesture(drag.undo_gesture()),
        )
    }
}
