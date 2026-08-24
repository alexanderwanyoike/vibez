//! Loop-brace interaction for the piano roll.

use iced::{Point, Rectangle};
use vibez_core::clip_timeline::BeatClipTimeline;

use crate::domains::piano_roll::PianoRollMsg;
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::widgets::clip_loop_markers::{self, LoopDrag, LoopMarker};

use super::{DragAction, PianoRollWidget, KEY_WIDTH};

const LOOP_HANDLE_ROW_HEIGHT: f32 = 10.0;

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
            LOOP_HANDLE_ROW_HEIGHT,
        )
    }

    pub(super) fn hit_test_start_marker(&self, position: Point, bounds: &Rectangle) -> bool {
        let Some(clip) = self.clip.as_ref() else {
            return false;
        };
        clip_loop_markers::hit_test_start(self.beat_to_x(clip.start_marker_beats, bounds), position)
    }

    pub(super) fn begin_marker_drag(
        &self,
        position: Point,
        bounds: &Rectangle,
    ) -> Option<DragAction> {
        let clip = self.clip.as_ref()?;
        if self.hit_test_start_marker(position, bounds) {
            Some(DragAction::Start {
                clip_id: clip.clip_id,
                undo_gesture: UndoGestureId::new(),
            })
        } else {
            let marker = self.hit_test_loop_marker(position, bounds)?;
            Some(DragAction::Loop {
                clip_id: clip.clip_id,
                drag: LoopDrag::begin(marker, clip.loop_start_beats, clip.loop_end_beats),
            })
        }
    }

    pub(super) fn marker_drag_message(
        &self,
        action: &DragAction,
        position: Point,
        bounds: &Rectangle,
    ) -> Option<Message> {
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
        match action {
            DragAction::Start {
                clip_id,
                undo_gesture,
            } => {
                let marker_step = min_length.min(self.total_beats).max(0.01);
                let start_marker_beats = BeatClipTimeline::new(
                    clip.start_marker_beats,
                    clip.loop_start_beats,
                    clip.loop_end_beats,
                    self.total_beats,
                    clip.loop_enabled,
                )
                .clamp_start_with_gap(pointer, 0.0, self.total_beats, marker_step);
                (start_marker_beats != clip.start_marker_beats).then(|| {
                    Message::PianoRoll(PianoRollMsg::SetNoteClipStartMarker {
                        track_id: self.track_id,
                        clip_id: *clip_id,
                        start_marker_beats,
                    })
                    .in_undo_gesture(*undo_gesture)
                })
            }
            DragAction::Loop { clip_id, drag } => {
                let (loop_start_beats, loop_end_beats) =
                    drag.resolve(pointer, min_length.min(self.total_beats), self.total_beats);
                ((loop_start_beats, loop_end_beats) != (clip.loop_start_beats, clip.loop_end_beats))
                    .then(|| {
                        Message::PianoRoll(PianoRollMsg::SetNoteClipLoopRegion {
                            track_id: self.track_id,
                            clip_id: *clip_id,
                            loop_start_beats,
                            loop_end_beats,
                        })
                        .in_undo_gesture(drag.undo_gesture())
                    })
            }
            _ => None,
        }
    }
}
