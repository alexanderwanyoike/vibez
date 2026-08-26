//! Pointer gestures that can own the per-track Clip canvas.

use crate::state::UndoGestureId;
use vibez_core::id::ClipId;

use super::fade_drag::{FadeClipDrag, FadeCurveDrag};

#[derive(Debug, Clone)]
pub enum ClipDragAction {
    MoveClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        is_note_clip: bool,
        start_local_x: f32,
        start_scroll_beats: f64,
        original_position_beats: f64,
        start_y: f32,
    },
    ResizeClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        is_note_clip: bool,
        clip_start_beat: f64,
    },
    FadeClip(FadeClipDrag),
    FadeCurve(FadeCurveDrag),
    PendingSeek {
        beat: f64,
        start_x: f32,
        anchor_column_y: f32,
    },
    RegionSelect {
        anchor_beat: f64,
        anchor_column_y: f32,
    },
    PanViewport {
        start_local_x: f32,
        start_scroll_beats: f64,
    },
}
