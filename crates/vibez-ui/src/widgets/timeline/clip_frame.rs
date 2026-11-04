//! The frame every timeline clip shares: its box, edge and title.

use iced::border::Radius;
use iced::widget::canvas;
use iced::{Point, Rectangle, Size};

use super::clip_colors::ClipColors;
use super::clip_title::draw_clip_title;
use super::CLIP_TITLE_HEIGHT;

/// Lane left showing after each clip, so back-to-back clips of one track
/// still read as separate clips when zoomed out to the whole song.
const CLIP_GAP: f32 = 1.0;

const SELECTION_WIDTH: f32 = 1.0;

/// The rectangle a clip's body, contents and border are painted in.
pub(super) fn clip_box(clip_x: f32, clip_y: f32, clip_w: f32, clip_h: f32) -> Rectangle {
    Rectangle {
        x: clip_x,
        y: clip_y,
        width: (clip_w - CLIP_GAP).max(1.0),
        height: clip_h,
    }
}

/// The path rectangle for a border of `stroke_width` around `body`. Strokes
/// are centred on the path, so it is inset by half the stroke to keep the
/// border out of the gap.
fn clip_outline(body: Rectangle, stroke_width: f32) -> Rectangle {
    let inset = stroke_width / 2.0;
    Rectangle {
        x: body.x + inset,
        y: body.y + inset,
        width: (body.width - stroke_width).max(0.0),
        height: (body.height - stroke_width).max(0.0),
    }
}

/// The on-screen part of a clip box and its corner radii.
pub(super) fn visible_clip(body: Rectangle, viewport_width: f32) -> (Rectangle, Radius) {
    let _ = viewport_width;
    (body, Radius::default())
}

/// Where a looped clip's passes after the first begin, on screen.
pub(super) fn loop_tick_xs(
    body: Rectangle,
    first_wrap_px: f32,
    pass_px: f32,
    viewport_width: f32,
) -> Vec<f32> {
    let _ = (body, first_wrap_px, pass_px, viewport_width);
    Vec::new()
}

/// Paint a clip's title strip and see-through body, under its contents.
pub(super) fn draw_clip_back(frame: &mut canvas::Frame, body: Rectangle, colors: &ClipColors) {
    let title_height = CLIP_TITLE_HEIGHT.min(body.height);
    frame.fill_rectangle(
        body.position(),
        Size::new(body.width, title_height),
        colors.title,
    );
    frame.fill_rectangle(
        Point::new(body.x, body.y + title_height),
        Size::new(body.width, body.height - title_height),
        colors.body,
    );
}

/// Paint a clip's selection edge and name over its contents.
pub(super) fn draw_clip_front(
    frame: &mut canvas::Frame,
    body: Rectangle,
    colors: &ClipColors,
    name: &str,
    looped: bool,
    viewport_width: f32,
) {
    if let Some(selection) = colors.selection {
        let outline = clip_outline(body, SELECTION_WIDTH);
        frame.stroke(
            &canvas::Path::rectangle(outline.position(), outline.size()),
            canvas::Stroke::default()
                .with_color(selection)
                .with_width(SELECTION_WIDTH),
        );
    }

    draw_clip_title(frame, name, body, looped, colors.text, viewport_width);
}

#[cfg(test)]
mod tests {
    use super::{clip_box, clip_outline, loop_tick_xs, visible_clip};
    use iced::Rectangle;

    fn clip(x: f32, width: f32) -> Rectangle {
        Rectangle {
            x,
            y: 2.0,
            width,
            height: 60.0,
        }
    }

    #[test]
    fn a_clip_on_screen_rounds_all_four_corners() {
        let (shown, radius) = visible_clip(clip(100.0, 200.0), 1000.0);

        assert_eq!(shown, clip(100.0, 200.0));
        assert_eq!(
            <[f32; 4]>::from(radius),
            [3.0, 3.0, 3.0, 3.0],
            "top left, top right, bottom right, bottom left"
        );
    }

    #[test]
    fn a_clip_cut_by_the_viewport_keeps_square_corners_where_it_is_cut() {
        let (shown, radius) = visible_clip(clip(-5000.0, 10_000.0), 1000.0);

        assert!(shown.x < 0.0 && shown.x > -10.0, "{shown:?}");
        assert!(shown.x + shown.width > 1000.0 && shown.x + shown.width < 1010.0);
        assert_eq!(<[f32; 4]>::from(radius), [0.0; 4]);

        let (_, left_cut) = visible_clip(clip(-50.0, 200.0), 1000.0);
        assert_eq!(<[f32; 4]>::from(left_cut), [0.0, 3.0, 3.0, 0.0]);
    }

    #[test]
    fn a_looped_clip_marks_where_each_later_pass_begins() {
        assert_eq!(
            loop_tick_xs(clip(100.0, 200.0), 40.0, 40.0, 1000.0),
            [140.0, 180.0, 220.0, 260.0]
        );
    }

    #[test]
    fn no_loop_ticks_when_passes_are_too_narrow_to_read() {
        assert!(loop_tick_xs(clip(100.0, 200.0), 6.0, 6.0, 1000.0).is_empty());
    }

    #[test]
    fn loop_ticks_are_made_only_for_the_visible_part_of_a_long_clip() {
        let ticks = loop_tick_xs(clip(-100_000.0, 200_000.0), 50.0, 50.0, 1000.0);

        assert_eq!(ticks.len(), 20);
        assert!(ticks.iter().all(|x| (0.0..1000.0).contains(x)), "{ticks:?}");
    }

    #[test]
    fn back_to_back_clips_leave_a_gap_between_them_at_any_zoom() {
        for clip_w in [400.0, 12.0, 3.0] {
            let first = clip_box(100.0, 4.0, clip_w, 70.0);
            let second = clip_box(100.0 + clip_w, 4.0, clip_w, 70.0);
            assert!(
                first.x + first.width + 1.0 <= second.x,
                "clips {clip_w} px wide"
            );
            assert!(first.width >= 1.0, "clips {clip_w} px wide");
        }
    }

    #[test]
    fn a_clip_border_never_paints_outside_its_box() {
        let body = clip_box(100.0, 4.0, 50.0, 70.0);
        for stroke in [1.0, 2.0] {
            let outline = clip_outline(body, stroke);
            assert!(outline.x - stroke / 2.0 >= body.x);
            assert!(outline.y - stroke / 2.0 >= body.y);
            assert!(outline.x + outline.width + stroke / 2.0 <= body.x + body.width);
            assert!(outline.y + outline.height + stroke / 2.0 <= body.y + body.height);
        }
    }
}
