//! The minutes and seconds scale along the bottom of the Arrange lanes.

use iced::widget::canvas;
use iced::{mouse, Point, Rectangle, Renderer, Size, Theme};

use crate::message::Message;
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;
use crate::typography;

pub const TIME_RULER_HEIGHT: f32 = 20.0;

/// Second steps a label may take, smallest first.
const SECOND_STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
/// Labels any closer collide at the label size.
const MIN_LABEL_GAP_PX: f64 = 64.0;
const TICK_HEIGHT: f32 = 6.0;

/// Canvas widget that labels the Arrange timeline in minutes and seconds.
pub struct TimeRulerWidget {
    pub bpm: f64,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
}

impl canvas::Program<Message> for TimeRulerWidget {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::ruler_bg());
        frame.fill_rectangle(Point::ORIGIN, Size::new(bounds.width, 1.0), theme::border());
        let geometry = TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats);
        for (x, label) in time_marks(geometry, bounds.width, self.bpm) {
            frame.fill_rectangle(
                Point::new(x, 0.0),
                Size::new(1.0, TICK_HEIGHT),
                theme::ruler_tick(),
            );
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(x + 4.0, 4.0),
                color: theme::text_dim(),
                size: iced::Pixels(10.5),
                font: typography::PERFORM_DISPLAY,
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

/// Labels across `width` pixels as whole-pixel x and "m:ss" text.
fn time_marks(geometry: TimelineGeometry, width: f32, bpm: f64) -> Vec<(f32, String)> {
    if bpm <= 0.0 {
        return Vec::new();
    }
    let beats_per_second = bpm / 60.0;
    let pixels_per_second = f64::from(geometry.pixels_per_beat()) * beats_per_second;
    let step = SECOND_STEPS
        .into_iter()
        .find(|step| step * pixels_per_second >= MIN_LABEL_GAP_PX)
        .unwrap_or(SECOND_STEPS[SECOND_STEPS.len() - 1]);
    let first = (geometry.x_to_beat(0.0).max(0.0) / beats_per_second / step).ceil() as i64;
    let last = (geometry.x_to_beat(width) / beats_per_second / step).floor() as i64;
    (first..=last)
        .filter_map(|index| {
            let seconds = index as f64 * step;
            let x = geometry.beat_to_x(seconds * beats_per_second).round();
            let whole = seconds.round() as u64;
            (0.0..=width)
                .contains(&x)
                .then(|| (x, format!("{}:{:02}", whole / 60, whole % 60)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_labelled_at_least_sixty_four_pixels_apart() {
        // 120 BPM at 20 px per beat is 40 px per second.
        let marks = time_marks(TimelineGeometry::new(20.0, 0.0), 170.0, 120.0);

        assert_eq!(
            marks,
            vec![
                (0.0, "0:00".to_string()),
                (80.0, "0:02".to_string()),
                (160.0, "0:04".to_string())
            ]
        );
    }

    #[test]
    fn labels_read_minutes_and_seconds_on_whole_pixels_when_scrolled() {
        // 128.3 beats in at 120 BPM is just past 1:04; labels step by 2 s.
        let marks = time_marks(TimelineGeometry::new(20.0, 128.3), 100.0, 120.0);

        assert_eq!(marks, vec![(74.0, "1:06".to_string())]);
    }

    #[test]
    fn zoomed_out_labels_step_by_minutes() {
        // One pixel per second labels every two minutes.
        let marks = time_marks(TimelineGeometry::new(0.5, 0.0), 300.0, 120.0);

        assert_eq!(
            marks,
            vec![
                (0.0, "0:00".to_string()),
                (120.0, "2:00".to_string()),
                (240.0, "4:00".to_string())
            ]
        );
    }
}
