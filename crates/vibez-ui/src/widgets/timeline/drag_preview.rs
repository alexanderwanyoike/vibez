//! A Clip drag's preview on one lane: the outline where the Clip began, the
//! guide at the line it snapped to, the readout beside the pointer and the
//! shadow that lifts the dragged Clip.

use iced::alignment::Vertical;
use iced::border::Radius;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Size};

use crate::theme;
use crate::widgets::device_controls::text_width;

use super::clip_frame::clip_box;
use super::{TrackClipCanvas, CLIP_Y};

const READOUT_SIZE: f32 = 11.0;
const READOUT_PADDING_X: f32 = 8.0;
const READOUT_HEIGHT: f32 = 19.0;
/// How far right of, and above, the pointer the readout sits, clear of the
/// cursor and of the edge being dragged.
const READOUT_OFFSET: (f32, f32) = (14.0, 30.0);
const READOUT_MARGIN: f32 = 2.0;
/// The readout is the same dark pill in every theme, as in the mockup, so
/// it reads over light and dark lanes alike.
const READOUT_FILL: Color = Color::from_rgba(0.04, 0.04, 0.05, 0.92);
const READOUT_TEXT: Color = Color::from_rgb(0.95, 0.95, 0.953);
const OUTLINE_DASH: [f32; 2] = [4.0, 3.0];
/// Rings outside the dragged Clip, nearest first, as (distance, opacity).
/// Rings rather than a fill, because the Clip's body is see-through.
const SHADOW_RINGS: [(f32, f32); 3] = [(0.5, 0.22), (1.5, 0.12), (2.5, 0.05)];
const SHADOW_DROP: f32 = 1.0;

/// Where the readout pill sits on a lane `lane_height` tall whose top is at
/// column y `lane_top`, or `None` when the pointer is on another lane.
pub(super) fn readout_box(
    pointer: Point,
    text_width: f32,
    lane_top: f32,
    lane_width: f32,
    lane_height: f32,
) -> Option<Rectangle> {
    let local_y = pointer.y - lane_top;
    if !(0.0..lane_height).contains(&local_y) {
        return None;
    }
    let size = Size::new(text_width + 2.0 * READOUT_PADDING_X, READOUT_HEIGHT);
    let x = (pointer.x + READOUT_OFFSET.0)
        .min(lane_width - size.width - READOUT_MARGIN)
        .max(READOUT_MARGIN);
    let y = (local_y - READOUT_OFFSET.1)
        .min(lane_height - size.height - READOUT_MARGIN)
        .max(READOUT_MARGIN);
    Some(Rectangle::new(Point::new(x.round(), y.round()), size))
}

impl TrackClipCanvas {
    /// The outline where the dragged Clip began, drawn under the Clips so
    /// the dragged one covers it wherever they overlap.
    pub(super) fn draw_drag_origin(&self, frame: &mut canvas::Frame, height: f32) {
        let Some(step) = self
            .clip_drag
            .as_ref()
            .map(|preview| &preview.step)
            .filter(|step| step.origin_track == self.track_id)
        else {
            return;
        };
        let x = self.beat_to_x(step.origin.start_beats);
        let w = self.geometry().width_for_beats(step.origin.length_beats);
        let body = clip_box(x, CLIP_Y, w, height - 2.0 * CLIP_Y);
        frame.stroke(
            &canvas::Path::rectangle(
                Point::new(body.x + 0.5, body.y + 0.5),
                Size::new(body.width - 1.0, body.height - 1.0),
            ),
            canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &OUTLINE_DASH,
                    offset: 0,
                },
                ..canvas::Stroke::default()
                    .with_color(theme::with_alpha(theme::text(), 0.45))
                    .with_width(1.0)
            },
        );
    }

    /// The guide at the line the drag snapped to, across every lane.
    pub(super) fn draw_drag_guide(&self, frame: &mut canvas::Frame, width: f32, height: f32) {
        let Some(guide) = self
            .clip_drag
            .as_ref()
            .and_then(|preview| preview.step.guide)
        else {
            return;
        };
        let x = self.beat_to_x(guide.beat).round();
        if (0.0..=width).contains(&x) {
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(1.0, height), theme::accent());
        }
    }

    /// The readout beside the pointer, drawn over everything else.
    pub(super) fn draw_drag_readout(&self, frame: &mut canvas::Frame, width: f32, height: f32) {
        let Some(preview) = self.clip_drag.as_ref().filter(|p| !p.readout.is_empty()) else {
            return;
        };
        let font = crate::typography::CLIP_TITLE;
        let Some(pill) = readout_box(
            preview.step.pointer,
            text_width(&preview.readout, READOUT_SIZE, font),
            self.row_top(),
            width,
            height,
        ) else {
            return;
        };
        frame.fill(
            &canvas::Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(4.0)),
            READOUT_FILL,
        );
        frame.fill_text(canvas::Text {
            content: preview.readout.clone(),
            position: Point::new(pill.x + READOUT_PADDING_X, pill.center_y()),
            color: READOUT_TEXT,
            size: READOUT_SIZE.into(),
            font,
            vertical_alignment: Vertical::Center,
            ..Default::default()
        });
    }

    /// Whether `clip_id` is the Clip being dragged, drawn last and lifted.
    pub(super) fn is_dragged(&self, clip_id: vibez_core::id::ClipId) -> bool {
        self.clip_drag
            .as_ref()
            .is_some_and(|preview| preview.step.anchor == clip_id)
    }

    /// `clips` with the dragged one last, so it paints over its neighbours.
    pub(super) fn dragged_last<'a, T>(
        &'a self,
        clips: &'a [T],
        clip_id: impl Fn(&T) -> vibez_core::id::ClipId + Copy + 'a,
    ) -> impl Iterator<Item = &'a T> + 'a {
        let resting = clips
            .iter()
            .filter(move |clip| !self.is_dragged(clip_id(clip)));
        let dragged = clips
            .iter()
            .filter(move |clip| self.is_dragged(clip_id(clip)));
        resting.chain(dragged)
    }
}

/// A soft shadow around the dragged Clip's `body`.
pub(super) fn draw_drag_shadow(frame: &mut canvas::Frame, body: Rectangle) {
    for (distance, opacity) in SHADOW_RINGS {
        frame.stroke(
            &canvas::Path::rounded_rectangle(
                Point::new(body.x - distance, body.y - distance + SHADOW_DROP),
                Size::new(body.width + 2.0 * distance, body.height + 2.0 * distance),
                Radius::from(3.0 + distance),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, opacity))
                .with_width(1.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::readout_box;
    use iced::Point;

    #[test]
    fn the_readout_sits_up_and_right_of_the_pointer_on_its_own_lane() {
        let pill = readout_box(Point::new(200.0, 150.0), 40.0, 100.0, 800.0, 70.0).unwrap();

        assert_eq!((pill.x, pill.y), (214.0, 20.0));
        assert_eq!(
            readout_box(Point::new(200.0, 90.0), 40.0, 100.0, 800.0, 70.0),
            None
        );
    }

    #[test]
    fn the_readout_never_leaves_its_lane() {
        let at_right = readout_box(Point::new(790.0, 105.0), 40.0, 100.0, 800.0, 70.0).unwrap();

        assert!(at_right.x + at_right.width <= 800.0);
        assert!(at_right.y >= 0.0);
    }
}
