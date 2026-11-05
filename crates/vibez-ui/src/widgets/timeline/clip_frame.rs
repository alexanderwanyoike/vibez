//! The frame every timeline clip shares, painted under and over its contents.

use iced::border::Radius;
use iced::widget::canvas;
use iced::{Point, Rectangle, Size};

use super::clip_colors::ClipColors;
use super::clip_title::draw_clip_title;
use super::{CLIP_TITLE_HEIGHT, CORNER_RADIUS};

/// Lane left showing after each clip, so back-to-back clips of one track
/// still read as separate clips when zoomed out to the whole song.
const CLIP_GAP: f32 = 1.0;

const SELECTION_WIDTH: f32 = 1.0;
/// How far past the viewport a cut clip is still painted, so its square cut
/// end never shows.
const OFFSCREEN_OVERHANG: f32 = 2.0;
/// Passes narrower than this would turn the ticks into a texture.
const MIN_LOOP_PASS_PX: f32 = 8.0;
/// A tick this close to the clip's start would read as part of its edge.
const LOOP_TICK_INSET: f32 = 2.0;

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

/// The on-screen part of a clip box and its corner radii. Only the clip's
/// real ends are rounded; where the viewport cuts it the corners stay square
/// and just off screen.
pub(super) fn visible_clip(body: Rectangle, viewport_width: f32) -> (Rectangle, Radius) {
    let body_right = body.x + body.width;
    let left = body.x.max(-OFFSCREEN_OVERHANG);
    let right = body_right.min(viewport_width + OFFSCREEN_OVERHANG);
    let left_radius = if left == body.x { CORNER_RADIUS } else { 0.0 };
    let right_radius = if right == body_right {
        CORNER_RADIUS
    } else {
        0.0
    };
    (
        Rectangle {
            x: left,
            width: (right - left).max(0.0),
            ..body
        },
        Radius {
            top_left: left_radius,
            top_right: right_radius,
            bottom_right: right_radius,
            bottom_left: left_radius,
        },
    )
}

/// Where a looped clip's passes after the first begin, on screen.
pub(super) fn loop_tick_xs(
    body: Rectangle,
    first_wrap_px: f32,
    pass_px: f32,
    viewport_width: f32,
) -> Vec<f32> {
    if pass_px < MIN_LOOP_PASS_PX {
        return Vec::new();
    }
    let first = body.x + first_wrap_px;
    let right = (body.x + body.width).min(viewport_width);
    let first_visible = (-first / pass_px).ceil().max(0.0) as usize;
    (first_visible..)
        .map(|pass| (first + pass as f32 * pass_px).round())
        .take_while(|x| *x < right)
        .filter(|x| *x >= body.x + LOOP_TICK_INSET)
        .collect()
}

/// A looped clip's passes in pixels: where the first repeat begins after
/// the clip start, and how long each pass is.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopPasses {
    pub first_wrap_px: f32,
    pub pass_px: f32,
}

/// Paint a clip's title strip, see-through body and loop-pass ticks, under
/// its contents.
pub(super) fn draw_clip_back(
    frame: &mut canvas::Frame,
    body: Rectangle,
    colors: &ClipColors,
    loop_passes: Option<LoopPasses>,
    viewport_width: f32,
) {
    let (shown, radius) = visible_clip(body, viewport_width);
    let title_height = CLIP_TITLE_HEIGHT.min(shown.height);
    frame.fill(
        &canvas::Path::rounded_rectangle(
            shown.position(),
            Size::new(shown.width, title_height),
            Radius {
                bottom_right: 0.0,
                bottom_left: 0.0,
                ..radius
            },
        ),
        colors.title,
    );
    let body_top = shown.y + title_height;
    let body_height = shown.height - title_height;
    frame.fill(
        &canvas::Path::rounded_rectangle(
            Point::new(shown.x, body_top),
            Size::new(shown.width, body_height),
            Radius {
                top_left: 0.0,
                top_right: 0.0,
                ..radius
            },
        ),
        colors.body,
    );
    if let Some(passes) = loop_passes {
        for x in loop_tick_xs(body, passes.first_wrap_px, passes.pass_px, viewport_width) {
            frame.fill_rectangle(
                Point::new(x, body_top),
                Size::new(1.0, body_height),
                colors.loop_tick,
            );
        }
    }
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
        let (shown, radius) = visible_clip(body, viewport_width);
        let outline = clip_outline(shown, SELECTION_WIDTH);
        let inset = SELECTION_WIDTH / 2.0;
        let [top_left, top_right, bottom_right, bottom_left] =
            <[f32; 4]>::from(radius).map(|corner| (corner - inset).max(0.0));
        frame.stroke(
            &canvas::Path::rounded_rectangle(
                outline.position(),
                outline.size(),
                Radius {
                    top_left,
                    top_right,
                    bottom_right,
                    bottom_left,
                },
            ),
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
