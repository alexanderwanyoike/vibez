//! The dashed zone that marks where a Browser drop will land and says what
//! it will do.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas;
use iced::{mouse, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme};

use crate::domains::drop_zone::{DropAction, DropZone};
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::text_width;
use crate::widgets::flat_stack::flat_stack;
use crate::widgets::top_layer::top_layer;

/// How far a zone sits inside a device card, so it reads as a place within
/// the card rather than the card's own edge.
pub const CARD_ZONE_INSET: f32 = 4.0;
/// A card zone's corners, round enough to read as rounded at its inset.
pub const CARD_ZONE_RADIUS: f32 = 6.0;
const STROKE_W: f32 = 1.5;
const DASH: f32 = 5.0;
const GAP: f32 = 4.0;
const LABEL_H: f32 = 26.0;
const LABEL_PAD_X: f32 = 11.0;
const ICON_SIZE: f32 = 13.0;
const ICON_GAP: f32 = 7.0;
const TEXT_SIZE: f32 = 12.0;
/// The least room the label keeps from the dashes on every side.
const LABEL_MARGIN: f32 = 8.0;

/// `content` with `zone` marked over it, held `inset` inside the content's
/// edges so it never covers chrome such as a scrollbar, its corners rounded
/// to `radius` there. The zone layer stays when there is no zone, so a hover never
/// rebuilds the content's widget state, and it sits on top so cards cannot
/// paint over it.
pub fn drop_highlight<'a>(
    content: Element<'a, Message>,
    zone: Option<DropZone>,
    radius: f32,
    inset: impl Into<Padding>,
) -> Element<'a, Message> {
    let size = content.as_widget().size_hint();
    let marks = canvas(ZoneMarks {
        zone,
        radius,
        inset: inset.into(),
    })
    .width(Length::Fill)
    .height(Length::Fill);
    flat_stack([content, top_layer(marks).into()])
        .width(size.width)
        .height(size.height)
        .into()
}

struct ZoneMarks {
    zone: Option<DropZone>,
    radius: f32,
    inset: Padding,
}

impl canvas::Program<Message> for ZoneMarks {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(zone) = &self.zone else {
            return Vec::new();
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The outer edge of the dashes lands on whole pixels, so a straight
        // run covers its pixels instead of smearing over a half one.
        let left = (bounds.x + self.inset.left).round() - bounds.x;
        let top = (bounds.y + self.inset.top).round() - bounds.y;
        let right = (bounds.x + bounds.width - self.inset.right).round() - bounds.x;
        let bottom = (bounds.y + bounds.height - self.inset.bottom).round() - bounds.y;
        let half = STROKE_W / 2.0;
        let line = Rectangle::new(
            Point::new(left + half, top + half),
            Size::new(right - left - STROKE_W, bottom - top - STROKE_W),
        );
        if line.width <= 0.0 || line.height <= 0.0 {
            return Vec::new();
        }
        let radius = (self.radius - half).max(STROKE_W);
        let outline = canvas::Path::rounded_rectangle(line.position(), line.size(), radius.into());
        frame.fill(&outline, th::drop_target_wash());
        let dashes = even_dash(rounded_perimeter(line.size(), radius), DASH, GAP);
        frame.stroke(
            &outline,
            canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &dashes,
                    offset: 0,
                },
                ..canvas::Stroke::default()
                    .with_color(th::accent())
                    .with_width(STROKE_W)
            },
        );
        if let Some(label) = &zone.label {
            draw_label(&mut frame, line, zone.action, label);
        }
        vec![frame.into_geometry()]
    }
}

/// The outcome centred in the zone on a backing of the card's surface, so
/// it reads over whatever controls sit under it.
fn draw_label(frame: &mut canvas::Frame, zone: Rectangle, action: DropAction, label: &str) {
    let font = iced::Font::default();
    let text_w = text_width(label, TEXT_SIZE, font);
    let size = Size::new(
        (LABEL_PAD_X * 2.0 + ICON_SIZE + ICON_GAP + text_w).round(),
        LABEL_H,
    );
    if !label_fits(zone.size(), size) {
        return;
    }
    let x = (zone.center_x() - size.width / 2.0).round();
    let y = (zone.center_y() - size.height / 2.0).round();
    let backing = canvas::Path::rounded_rectangle(
        Point::new(x + 0.5, y + 0.5),
        Size::new(size.width - 1.0, size.height - 1.0),
        (LABEL_H / 2.0).into(),
    );
    frame.fill(&backing, th::bg_elevated());
    frame.stroke(
        &backing,
        canvas::Stroke::default()
            .with_color(th::border_light())
            .with_width(1.0),
    );
    let mid = y + LABEL_H / 2.0;
    let glyph = match action {
        DropAction::Add => crate::icons::PLUS,
        DropAction::Replace => crate::icons::ARROW_RIGHT_LEFT,
        DropAction::Load => crate::icons::ARROW_DOWN_TO_LINE,
        DropAction::Refuse => crate::icons::BAN,
    };
    frame.fill_text(canvas::Text {
        content: glyph.to_string(),
        position: Point::new(x + LABEL_PAD_X, mid),
        color: th::accent(),
        size: ICON_SIZE.into(),
        font: crate::icons::ICON_FONT,
        horizontal_alignment: Horizontal::Left,
        vertical_alignment: Vertical::Center,
        ..Default::default()
    });
    frame.fill_text(canvas::Text {
        content: label.to_string(),
        position: Point::new(x + LABEL_PAD_X + ICON_SIZE + ICON_GAP, mid),
        color: th::text(),
        size: TEXT_SIZE.into(),
        font,
        horizontal_alignment: Horizontal::Left,
        vertical_alignment: Vertical::Center,
        ..Default::default()
    });
}

/// The length of a `size` rectangle's outline with corners of `radius`.
fn rounded_perimeter(size: Size, radius: f32) -> f32 {
    let radius = radius.min(size.width / 2.0).min(size.height / 2.0);
    2.0 * (size.width + size.height - 4.0 * radius) + std::f32::consts::TAU * radius
}

/// `dash` and `gap` stretched so a whole number of them goes round a
/// `perimeter`, so the pattern meets itself without a stub at the seam.
fn even_dash(perimeter: f32, dash: f32, gap: f32) -> [f32; 2] {
    let periods = (perimeter / (dash + gap)).round().max(1.0);
    let stretch = perimeter / (periods * (dash + gap));
    [dash * stretch, gap * stretch]
}

/// Whether a `label` sized backing fits a `zone` with room around it.
fn label_fits(zone: Size, label: Size) -> bool {
    zone.width >= label.width + LABEL_MARGIN * 2.0
        && zone.height >= label.height + LABEL_MARGIN * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    #[test]
    fn a_rounded_outline_is_its_sides_plus_one_circle_of_corners() {
        let perimeter = rounded_perimeter(Size::new(100.0, 40.0), 8.0);
        let expected = 2.0 * (100.0 - 16.0 + 40.0 - 16.0) + std::f32::consts::TAU * 8.0;
        assert!((perimeter - expected).abs() < 1e-3);
    }

    #[test]
    fn a_corner_never_rounds_past_half_the_short_side() {
        let pill = rounded_perimeter(Size::new(100.0, 10.0), 8.0);
        let expected = 2.0 * (100.0 - 10.0) + std::f32::consts::TAU * 5.0;
        assert!((pill - expected).abs() < 1e-3);
    }

    #[test]
    fn dashes_stretch_to_close_the_outline_without_a_stub() {
        let [dash, gap] = even_dash(103.0, 6.0, 4.0);
        let periods: f32 = 103.0 / (dash + gap);
        assert!((periods - periods.round()).abs() < 1e-4);
        assert_eq!(periods.round(), 10.0);
        assert!((dash / gap - 1.5).abs() < 1e-4);
    }

    #[test]
    fn an_outline_shorter_than_one_dash_still_gets_one() {
        let [dash, gap] = even_dash(4.0, 6.0, 4.0);
        assert!((dash + gap - 4.0).abs() < 1e-4);
    }

    #[test]
    fn the_label_shows_only_with_room_around_it() {
        let label = Size::new(120.0, 24.0);
        assert!(label_fits(Size::new(200.0, 60.0), label));
        assert!(!label_fits(Size::new(124.0, 60.0), label));
        assert!(!label_fits(Size::new(200.0, 26.0), label));
    }
}
