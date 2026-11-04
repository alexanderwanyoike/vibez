//! Shared visual language and hit testing for loop braces.

use iced::widget::canvas;
use iced::{Color, Point};

use crate::state::UndoGestureId;
use crate::widgets::span_edges::{nearest_edge, span_with_end, span_with_start, SpanEdge};

const HANDLE_WIDTH: f32 = 8.0;
const HIT_RADIUS: f32 = 8.0;
const START_MARKER_TOP: f32 = 10.5;
const START_MARKER_MIDDLE: f32 = 15.0;
const START_MARKER_BOTTOM: f32 = 19.5;
const START_MARKER_WIDTH: f32 = 7.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopMarker {
    Start,
    End,
    /// The bar between the handles, which carries the whole loop.
    Body,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LoopDrag {
    Start {
        fixed_end: f64,
        undo_gesture: UndoGestureId,
    },
    End {
        fixed_start: f64,
        undo_gesture: UndoGestureId,
    },
    Move {
        length: f64,
        /// How far the pointer pressed past the loop start.
        grab: f64,
        undo_gesture: UndoGestureId,
    },
}

impl LoopDrag {
    /// `pointer` is where the press landed, snapped as the drag will be.
    pub(crate) fn begin(marker: LoopMarker, start: f64, end: f64, pointer: f64) -> Self {
        match marker {
            LoopMarker::Start => Self::Start {
                fixed_end: end,
                undo_gesture: UndoGestureId::new(),
            },
            LoopMarker::End => Self::End {
                fixed_start: start,
                undo_gesture: UndoGestureId::new(),
            },
            LoopMarker::Body => Self::Move {
                length: end - start,
                grab: pointer - start,
                undo_gesture: UndoGestureId::new(),
            },
        }
    }

    pub(crate) fn resolve(self, pointer: f64, min_length: f64, max: f64) -> (f64, f64) {
        match self {
            Self::Start { fixed_end, .. } => span_with_start(pointer, fixed_end, min_length),
            Self::End { fixed_start, .. } => span_with_end(pointer, fixed_start, min_length, max),
            Self::Move { length, grab, .. } => {
                let start = (pointer - grab).clamp(0.0, (max - length).max(0.0));
                (start, start + length)
            }
        }
    }

    pub(crate) fn undo_gesture(self) -> UndoGestureId {
        match self {
            Self::Start { undo_gesture, .. }
            | Self::End { undo_gesture, .. }
            | Self::Move { undo_gesture, .. } => undo_gesture,
        }
    }
}

pub(crate) fn hit_test(
    start_x: f32,
    end_x: f32,
    position: Point,
    rail_height: f32,
) -> Option<LoopMarker> {
    if position.y < 0.0 || position.y > rail_height {
        return None;
    }
    nearest_end(start_x, end_x, position.x, HIT_RADIUS).or_else(|| {
        let inside = position.x > start_x.min(end_x) && position.x < start_x.max(end_x);
        inside.then_some(LoopMarker::Body)
    })
}

/// The end of a brace from `start_x` to `end_x` within `radius` of `x`,
/// the nearer one where both are.
pub(crate) fn nearest_end(start_x: f32, end_x: f32, x: f32, radius: f32) -> Option<LoopMarker> {
    nearest_edge(start_x, end_x, x, radius).map(|edge| match edge {
        SpanEdge::Start => LoopMarker::Start,
        SpanEdge::End => LoopMarker::End,
    })
}

pub(crate) fn draw_brace(frame: &mut canvas::Frame, start_x: f32, end_x: f32, color: Color) {
    if end_x == start_x {
        return;
    }
    let left = start_x.min(end_x);
    let right = start_x.max(end_x);

    frame.fill_rectangle(
        Point::new(left, 1.0),
        iced::Size::new(right - left, 3.0),
        color,
    );

    let start_inside = if start_x < end_x {
        start_x + HANDLE_WIDTH
    } else {
        start_x - HANDLE_WIDTH
    };
    let start_handle = canvas::Path::new(|path| {
        path.move_to(Point::new(start_x, 1.0));
        path.line_to(Point::new(start_inside, 1.0));
        path.line_to(Point::new(start_x, 10.0));
        path.close();
    });
    let end_inside = if end_x > start_x {
        end_x - HANDLE_WIDTH
    } else {
        end_x + HANDLE_WIDTH
    };
    let end_handle = canvas::Path::new(|path| {
        path.move_to(Point::new(end_x, 1.0));
        path.line_to(Point::new(end_inside, 1.0));
        path.line_to(Point::new(end_x, 10.0));
        path.close();
    });
    frame.fill(&start_handle, color);
    frame.fill(&end_handle, color);
}

pub(crate) fn hit_test_start(marker_x: f32, position: Point) -> bool {
    (START_MARKER_TOP..=START_MARKER_BOTTOM).contains(&position.y)
        && (position.x - marker_x).abs() <= HIT_RADIUS
}

/// Draw the one-shot Start marker as a quiet outlined tab. The shape carries
/// the meaning, so it does not need a permanent label competing with the ruler.
pub(crate) fn draw_start(
    frame: &mut canvas::Frame,
    marker_x: f32,
    line_end: f32,
    color: Color,
    fill: Color,
    points_right: bool,
) {
    let stem = canvas::Path::line(
        Point::new(marker_x, START_MARKER_BOTTOM),
        Point::new(marker_x, line_end),
    );
    frame.stroke(
        &stem,
        canvas::Stroke::default()
            .with_color(Color { a: 0.45, ..color })
            .with_width(1.0),
    );

    let handle = canvas::Path::new(|path| {
        let tip_x = if points_right {
            marker_x + START_MARKER_WIDTH
        } else {
            marker_x - START_MARKER_WIDTH
        };
        path.move_to(Point::new(marker_x, START_MARKER_TOP));
        path.line_to(Point::new(tip_x, START_MARKER_MIDDLE));
        path.line_to(Point::new(marker_x, START_MARKER_BOTTOM));
        path.close();
    });
    frame.fill(&handle, fill);
    frame.stroke(
        &handle,
        canvas::Stroke::default().with_color(color).with_width(1.0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_marker_wins_when_hit_targets_overlap() {
        assert_eq!(
            hit_test(100.0, 108.0, Point::new(102.0, 5.0), 20.0),
            Some(LoopMarker::Start)
        );
        assert_eq!(
            hit_test(100.0, 108.0, Point::new(106.0, 5.0), 20.0),
            Some(LoopMarker::End)
        );
    }

    #[test]
    fn marker_hit_target_stays_inside_the_rail() {
        assert_eq!(hit_test(100.0, 200.0, Point::new(100.0, 21.0), 20.0), None);
    }

    #[test]
    fn drag_respects_the_minimum_length_when_the_pointer_overshoots() {
        let start = LoopDrag::begin(LoopMarker::Start, 10.0, 20.0, 10.0);
        assert_eq!(start.resolve(30.0, 4.0, 40.0), (16.0, 20.0));

        let end = LoopDrag::begin(LoopMarker::End, 10.0, 20.0, 20.0);
        assert_eq!(end.resolve(0.0, 4.0, 40.0), (10.0, 14.0));
    }

    #[test]
    fn the_brace_between_its_handles_grabs_the_whole_loop() {
        assert_eq!(
            hit_test(100.0, 200.0, Point::new(150.0, 5.0), 10.0),
            Some(LoopMarker::Body)
        );
        assert_eq!(hit_test(100.0, 200.0, Point::new(250.0, 5.0), 10.0), None);
    }

    #[test]
    fn moving_the_whole_loop_keeps_its_length_inside_the_content() {
        let drag = LoopDrag::begin(LoopMarker::Body, 10.0, 20.0, 12.0);
        assert_eq!(drag.resolve(16.0, 4.0, 40.0), (14.0, 24.0));
        assert_eq!(drag.resolve(0.0, 4.0, 40.0), (0.0, 10.0));
        assert_eq!(drag.resolve(100.0, 4.0, 40.0), (30.0, 40.0));
    }
}
