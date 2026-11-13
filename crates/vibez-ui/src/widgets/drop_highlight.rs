//! The dashed zone that marks where a Browser drop will land and says what
//! it will do.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas;
use iced::{mouse, Color, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme};

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
/// The least room the label keeps from the drag badge, so the two never
/// read as one crowded message.
const LABEL_GAP: f32 = 10.0;

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
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(zone) = &self.zone else {
            return Vec::new();
        };
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let area = Rectangle::new(
            Point::new(self.inset.left, self.inset.top),
            Size::new(
                bounds.width - self.inset.left - self.inset.right,
                bounds.height - self.inset.top - self.inset.bottom,
            ),
        );
        let cursor = cursor
            .position()
            .map(|at| Point::new(at.x - bounds.x, at.y - bounds.y));
        draw_zone(
            &mut frame,
            bounds.position(),
            area,
            self.radius,
            zone,
            cursor,
        );
        vec![frame.into_geometry()]
    }
}

/// Marks `zone` over `area` of `frame`, whose top left sits at `origin` in
/// the window, with corners of `radius` and the label kept clear of the
/// drag badge at `cursor`, both in the frame's coordinates.
pub fn draw_zone(
    frame: &mut canvas::Frame,
    origin: Point,
    area: Rectangle,
    radius: f32,
    zone: &DropZone,
    cursor: Option<Point>,
) {
    // The outer edge of the dashes lands on whole pixels, so a straight run
    // covers its pixels instead of smearing over a half one.
    let left = (origin.x + area.x).round() - origin.x;
    let top = (origin.y + area.y).round() - origin.y;
    let right = (origin.x + area.x + area.width).round() - origin.x;
    let bottom = (origin.y + area.y + area.height).round() - origin.y;
    let half = STROKE_W / 2.0;
    let line = Rectangle::new(
        Point::new(left + half, top + half),
        Size::new(right - left - STROKE_W, bottom - top - STROKE_W),
    );
    if line.width <= 0.0 || line.height <= 0.0 {
        return;
    }
    let tone = if zone.action.refuses() {
        th::danger()
    } else {
        th::accent()
    };
    let radius = (radius - half).max(STROKE_W);
    let outline = canvas::Path::rounded_rectangle(line.position(), line.size(), radius.into());
    frame.fill(&outline, th::drop_zone_wash(tone));
    let dashes = even_dash(rounded_perimeter(line.size(), radius), DASH, GAP);
    frame.stroke(
        &outline,
        canvas::Stroke {
            line_dash: canvas::LineDash {
                segments: &dashes,
                offset: 0,
            },
            ..canvas::Stroke::default()
                .with_color(tone)
                .with_width(STROKE_W)
        },
    );
    if let Some(label) = &zone.label {
        let badge = cursor.map(crate::widgets::browser_drag_ghost::reserved_around);
        draw_label(frame, line, zone.action, tone, label, badge);
    }
}

/// The outcome in the zone on a backing of the card's surface, so it reads
/// over whatever controls sit under it, centred unless the drag badge needs
/// the room.
fn draw_label(
    frame: &mut canvas::Frame,
    zone: Rectangle,
    action: DropAction,
    tone: Color,
    label: &str,
    badge: Option<Rectangle>,
) {
    let font = iced::Font::default();
    let text_w = text_width(label, TEXT_SIZE, font);
    let size = Size::new(
        (LABEL_PAD_X * 2.0 + ICON_SIZE + ICON_GAP + text_w).round(),
        LABEL_H,
    );
    let Some(origin) = label_origin(zone, size, badge) else {
        return;
    };
    let (x, y) = (origin.x.round(), origin.y.round());
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
        color: tone,
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

/// Where a `label` sized backing goes in `zone`: centred, or moved straight
/// up or down until it keeps `LABEL_GAP` from the `badge` by the cursor,
/// whichever stays nearer the centre. `None` where it fits nowhere, since a
/// label under the badge would read as two competing messages.
fn label_origin(zone: Rectangle, label: Size, badge: Option<Rectangle>) -> Option<Point> {
    if !label_fits(zone.size(), label) {
        return None;
    }
    let x = zone.center_x() - label.width / 2.0;
    let centred = zone.center_y() - label.height / 2.0;
    let Some(badge) = badge else {
        return Some(Point::new(x, centred));
    };
    let clear = |y: f32| {
        let grown = Rectangle {
            x: x - LABEL_GAP,
            y: y - LABEL_GAP,
            width: label.width + LABEL_GAP * 2.0,
            height: label.height + LABEL_GAP * 2.0,
        };
        grown.intersection(&badge).is_none()
    };
    if clear(centred) {
        return Some(Point::new(x, centred));
    }
    let lowest = zone.y + zone.height - LABEL_MARGIN - label.height;
    let highest = zone.y + LABEL_MARGIN;
    [
        badge.y - LABEL_GAP - label.height,
        badge.y + badge.height + LABEL_GAP,
    ]
    .into_iter()
    .filter(|y| (highest..=lowest).contains(y) && clear(*y))
    .min_by(|a, b| (a - centred).abs().total_cmp(&(b - centred).abs()))
    .map(|y| Point::new(x, y))
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

    use iced::{Point, Rectangle};

    const ZONE: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 120.0,
    };
    const LABEL: Size = Size {
        width: 200.0,
        height: 26.0,
    };

    fn badge_at(x: f32, y: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(230.0, 40.0))
    }

    /// The label stays inside the zone and keeps its gap from the badge.
    fn assert_clear(origin: Point, avoid: Rectangle) {
        let label = Rectangle::new(origin, LABEL);
        assert!(label.y >= ZONE.y + LABEL_MARGIN);
        assert!(label.y + label.height <= ZONE.y + ZONE.height - LABEL_MARGIN);
        let grown = Rectangle {
            x: label.x - LABEL_GAP,
            y: label.y - LABEL_GAP,
            width: label.width + LABEL_GAP * 2.0,
            height: label.height + LABEL_GAP * 2.0,
        };
        assert!(grown.intersection(&avoid).is_none());
    }

    #[test]
    fn the_label_sits_centred_while_the_cursor_is_elsewhere() {
        let avoid = badge_at(330.0, 100.0);
        assert_eq!(
            label_origin(ZONE, LABEL, Some(avoid)),
            Some(Point::new(100.0, 47.0))
        );
        assert_eq!(
            label_origin(ZONE, LABEL, None),
            Some(Point::new(100.0, 47.0))
        );
    }

    #[test]
    fn the_label_yields_upward_from_a_cursor_at_the_centre() {
        let avoid = badge_at(200.0, 60.0);
        let origin = label_origin(ZONE, LABEL, Some(avoid)).unwrap();
        assert_eq!(origin.x, 100.0);
        assert!(origin.y < 47.0);
        assert_clear(origin, avoid);
    }

    #[test]
    fn the_label_yields_downward_when_there_is_no_room_above() {
        let avoid = badge_at(200.0, 20.0);
        let origin = label_origin(ZONE, LABEL, Some(avoid)).unwrap();
        assert!(origin.y > 47.0);
        assert_clear(origin, avoid);
    }

    #[test]
    fn the_label_steps_aside_when_neither_side_has_room() {
        let short = Rectangle {
            height: 60.0,
            ..ZONE
        };
        assert_eq!(
            label_origin(short, LABEL, Some(badge_at(200.0, 20.0))),
            None
        );
    }

    #[test]
    fn a_label_too_big_for_its_zone_is_not_shown() {
        let pad = Rectangle::new(Point::ORIGIN, Size::new(60.0, 24.0));
        assert_eq!(label_origin(pad, LABEL, None), None);
    }
}
