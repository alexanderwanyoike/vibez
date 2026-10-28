//! Pointer gestures that can own the per-track Clip canvas.

use crate::state::{ArrangementSelection, UndoGestureId};
use vibez_core::id::ClipId;

use super::fade_drag::{CrossfadeCurveDrag, FadeClipDrag, FadeCurveDrag};

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
        /// Set when the press landed on an already-selected Clip: the
        /// selection stays for a group drag, and a click without a drag
        /// narrows it to this Clip on release.
        collapse_to: Option<ArrangementSelection>,
    },
    ResizeClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        is_note_clip: bool,
        clip_start_beat: f64,
    },
    FadeClip(FadeClipDrag),
    FadeCurve(FadeCurveDrag),
    CrossfadeCurve(CrossfadeCurveDrag),
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
