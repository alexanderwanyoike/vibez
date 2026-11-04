//! The frame every timeline clip shares: its box, edge and title.

use iced::widget::canvas;
use iced::{Color, Rectangle};

use crate::theme;

use super::clip_title::draw_clip_title;
use super::CLIP_TITLE_HEIGHT;

/// Lane left showing after each clip, so back-to-back clips of one track
/// still read as separate clips when zoomed out to the whole song.
const CLIP_GAP: f32 = 1.0;

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

/// Paint a clip's edge, title separator and name over its contents.
pub(super) fn draw_clip_front(
    frame: &mut canvas::Frame,
    body: Rectangle,
    edge: Color,
    edge_width: f32,
    name: &str,
    looped: bool,
    viewport_width: f32,
) {
    let outline = clip_outline(body, edge_width);
    frame.stroke(
        &canvas::Path::rectangle(outline.position(), outline.size()),
        canvas::Stroke::default()
            .with_color(edge)
            .with_width(edge_width),
    );

    let title_sep_y = body.y + CLIP_TITLE_HEIGHT;
    frame.stroke(
        &canvas::Path::line(
            iced::Point::new(body.x, title_sep_y),
            iced::Point::new(body.x + body.width, title_sep_y),
        ),
        canvas::Stroke::default()
            .with_color(theme::with_alpha(Color::BLACK, 0.3))
            .with_width(1.0),
    );

    draw_clip_title(
        frame,
        name,
        body.x,
        body.y,
        body.width,
        looped,
        viewport_width,
    );
}

#[cfg(test)]
mod tests {
    use super::{clip_box, clip_outline};

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
