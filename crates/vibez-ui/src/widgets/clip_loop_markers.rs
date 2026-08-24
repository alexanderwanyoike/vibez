//! Shared visual language and hit testing for Clip loop braces.

use iced::widget::canvas;
use iced::{Color, Point};

use crate::state::UndoGestureId;

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
}

impl LoopDrag {
    pub(crate) fn begin(marker: LoopMarker, start: f64, end: f64) -> Self {
        match marker {
            LoopMarker::Start => Self::Start {
                fixed_end: end,
                undo_gesture: UndoGestureId::new(),
            },
            LoopMarker::End => Self::End {
                fixed_start: start,
                undo_gesture: UndoGestureId::new(),
            },
        }
    }

    pub(crate) fn resolve(self, pointer: f64, min_length: f64, max: f64) -> (f64, f64) {
        match self {
            Self::Start { fixed_end, .. } => (
                pointer.clamp(0.0, (fixed_end - min_length).max(0.0)),
                fixed_end,
            ),
            Self::End { fixed_start, .. } => (
                fixed_start,
                pointer.clamp((fixed_start + min_length).min(max), max),
            ),
        }
    }

    pub(crate) fn undo_gesture(self) -> UndoGestureId {
        match self {
            Self::Start { undo_gesture, .. } | Self::End { undo_gesture, .. } => undo_gesture,
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

    let start_distance = (position.x - start_x).abs();
    let end_distance = (position.x - end_x).abs();
    let nearest = if start_distance <= end_distance {
        (LoopMarker::Start, start_distance)
    } else {
        (LoopMarker::End, end_distance)
    };
    (nearest.1 <= HIT_RADIUS).then_some(nearest.0)
}

pub(crate) fn draw_brace(frame: &mut canvas::Frame, start_x: f32, end_x: f32, color: Color) {
    if end_x <= start_x {
        return;
    }

    frame.fill_rectangle(
        Point::new(start_x, 1.0),
        iced::Size::new(end_x - start_x, 3.0),
        color,
    );

    let start_handle = canvas::Path::new(|path| {
        path.move_to(Point::new(start_x, 1.0));
        path.line_to(Point::new(start_x + HANDLE_WIDTH, 1.0));
        path.line_to(Point::new(start_x, 10.0));
        path.close();
    });
    let end_handle = canvas::Path::new(|path| {
        path.move_to(Point::new(end_x, 1.0));
        path.line_to(Point::new(end_x - HANDLE_WIDTH, 1.0));
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
        path.move_to(Point::new(marker_x, START_MARKER_TOP));
        path.line_to(Point::new(
            marker_x + START_MARKER_WIDTH,
            START_MARKER_MIDDLE,
        ));
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
        let start = LoopDrag::begin(LoopMarker::Start, 10.0, 20.0);
        assert_eq!(start.resolve(30.0, 4.0, 40.0), (16.0, 20.0));

        let end = LoopDrag::begin(LoopMarker::End, 10.0, 20.0);
        assert_eq!(end.resolve(0.0, 4.0, 40.0), (10.0, 14.0));
    }
}
