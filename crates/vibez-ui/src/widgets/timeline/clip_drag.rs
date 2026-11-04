//! Pointer gestures that can own the per-track Clip canvas.

use crate::state::{ArrangementSelection, ClipSpan, UndoGestureId};
use crate::widgets::drag_phase::DragPhase;
use vibez_core::id::ClipId;

use super::fade_drag::{CrossfadeCurveDrag, FadeClipDrag, FadeCurveDrag};

/// The edge of a Clip a resize drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipEdge {
    Start,
    End,
}

impl ClipEdge {
    /// The beat of `span` that stays put while this edge is dragged.
    pub fn fixed_beat(self, span: ClipSpan) -> f64 {
        match self {
            Self::Start => span.end_beats(),
            Self::End => span.start_beats,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ClipDragAction {
    MoveClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        start_local_x: f32,
        start_scroll_beats: f64,
        origin: ClipSpan,
        start_y: f32,
        /// Set when the press landed on an already-selected Clip: the
        /// selection stays for a group drag, and a click without a drag
        /// narrows it to this Clip on release.
        collapse_to: Option<ArrangementSelection>,
        /// Pending until the pointer travels past the click slop, so a
        /// shaky click never moves the Clip.
        phase: DragPhase,
    },
    ResizeClip {
        undo_gesture: UndoGestureId,
        anchor: ArrangementSelection,
        edge: ClipEdge,
        origin: ClipSpan,
        start_local_x: f32,
        /// Pending until the pointer travels past the click slop, so a
        /// shaky click never changes the Clip's length.
        phase: DragPhase,
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
