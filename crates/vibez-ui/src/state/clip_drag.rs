//! A Clip drag in flight on a timeline.

use std::collections::HashSet;

use iced::Point;
use vibez_core::id::{ClipId, TrackId};

use super::{ArrangementSelection, ClipSpan, UndoGestureId};
use crate::timeline_snap::SnapTarget;

/// The part of a Clip a drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipDragKind {
    Move,
    Resize(ClipEdge),
}

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

/// One pointer step of a Clip drag, as the lane it began on saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipDragStep {
    pub gesture: UndoGestureId,
    pub kind: ClipDragKind,
    pub anchor: ClipId,
    pub origin_track: TrackId,
    pub origin: ClipSpan,
    /// The line the dragged edge locked onto; `None` when it moved freely.
    pub guide: Option<SnapTarget>,
    /// Lane x and column y, which every lane of a column shares.
    pub pointer: Point,
}

/// What every lane draws for the latest step of a Clip drag.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipDragPreview {
    pub step: ClipDragStep,
    /// Where the anchor now sits, such as "33.1.1 to Bass" or "Length 4.0.0".
    pub readout: String,
}

/// The selection a timeline had before a Clip drag, which a cross-lane
/// move re-homes and a cancelled drag puts back.
#[derive(Debug, Clone, Default)]
pub struct EditorSelection {
    pub clips: HashSet<ArrangementSelection>,
    pub track: Option<TrackId>,
    pub note_clip: Option<(TrackId, ClipId)>,
}

/// The Clip drag in flight on one timeline.
#[derive(Debug, Clone)]
pub struct ClipDrag {
    pub preview: ClipDragPreview,
    pub selection_before: EditorSelection,
}
