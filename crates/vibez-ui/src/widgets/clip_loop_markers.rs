//! Shared visual language and hit testing for Clip loop braces.

use iced::widget::canvas;
use iced::{Color, Point};

pub(crate) const MARKER_RAIL_HEIGHT: f32 = 18.0;
const HANDLE_WIDTH: f32 = 8.0;
const HIT_RADIUS: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopMarker {
    Start,
    End,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_marker_wins_when_hit_targets_overlap() {
        assert_eq!(
            hit_test(100.0, 108.0, Point::new(102.0, 5.0), MARKER_RAIL_HEIGHT),
            Some(LoopMarker::Start)
        );
        assert_eq!(
            hit_test(100.0, 108.0, Point::new(106.0, 5.0), MARKER_RAIL_HEIGHT),
            Some(LoopMarker::End)
        );
    }

    #[test]
    fn marker_hit_target_stays_inside_the_rail() {
        assert_eq!(
            hit_test(
                100.0,
                200.0,
                Point::new(100.0, MARKER_RAIL_HEIGHT + 1.0),
                MARKER_RAIL_HEIGHT
            ),
            None
        );
    }
}
