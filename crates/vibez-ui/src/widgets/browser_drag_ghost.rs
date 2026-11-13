//! The badge at the cursor that names the sample a Browser drag carries.

use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::canvas;
use iced::{Font, Point, Rectangle, Renderer, Size, Theme};

use crate::message::Message;
use crate::theme;
use crate::widgets::device_controls::text_width;

const HEIGHT: f32 = 18.0;
const PAD_X: f32 = 7.0;
const DOT_R: f32 = 2.5;
const DOT_GAP: f32 = 5.0;
const TEXT_SIZE: f32 = 10.5;
/// Below and right of the pointer's tip, clear of the arrow, like an
/// operating system's drag badge.
const CURSOR_OFFSET: Point = Point::new(14.0, 18.0);
const EDGE: f32 = 8.0;
/// Keeps the badge compact however long the sample's name is.
const TITLE_MAX_W: f32 = 200.0;
const MAX_W: f32 = PAD_X * 2.0 + DOT_R * 2.0 + DOT_GAP + TITLE_MAX_W;
/// Room around the pointer's arrow kept with the badge.
const POINTER_ROOM: f32 = 4.0;

#[derive(Debug, Clone)]
pub struct BrowserDragGhost {
    pub cursor: Point,
    /// The sample being carried, its mode and length.
    pub title: String,
    pub valid: Option<bool>,
}

/// What the pointer at `cursor` and its badge may cover, for a drop zone's
/// label to keep clear of.
pub fn reserved_around(cursor: Point) -> Rectangle {
    Rectangle::new(
        Point::new(cursor.x - POINTER_ROOM, cursor.y - POINTER_ROOM),
        Size::new(
            CURSOR_OFFSET.x + MAX_W + POINTER_ROOM,
            CURSOR_OFFSET.y + HEIGHT + POINTER_ROOM,
        ),
    )
}

impl canvas::Program<Message> for BrowserDragGhost {
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
        let dot_color = match self.valid {
            Some(true) => theme::accent(),
            Some(false) => theme::danger(),
            None => theme::text_muted(),
        };
        let font = Font::default();
        let fixed = PAD_X * 2.0 + DOT_R * 2.0 + DOT_GAP;
        let title = crate::typography::fit_text_middle(&self.title, TITLE_MAX_W, TEXT_SIZE, font);
        let width = (fixed + text_width(&title, TEXT_SIZE, font)).round();

        let x = (self.cursor.x + CURSOR_OFFSET.x)
            .min((bounds.width - width - EDGE).max(EDGE))
            .round();
        let y = (self.cursor.y + CURSOR_OFFSET.y)
            .min((bounds.height - HEIGHT - EDGE).max(EDGE))
            .round();
        let badge = canvas::Path::rounded_rectangle(
            Point::new(x + 0.5, y + 0.5),
            Size::new(width - 1.0, HEIGHT - 1.0),
            (HEIGHT / 2.0).into(),
        );
        frame.fill(&badge, theme::bg_elevated());
        frame.stroke(
            &badge,
            canvas::Stroke::default()
                .with_color(theme::border_light())
                .with_width(1.0),
        );
        let mid = y + HEIGHT / 2.0;
        frame.fill(
            &canvas::Path::circle(Point::new(x + PAD_X + DOT_R, mid), DOT_R),
            dot_color,
        );
        frame.fill_text(canvas::Text {
            content: title,
            position: Point::new(x + PAD_X + DOT_R * 2.0 + DOT_GAP, mid),
            color: theme::text_dim(),
            size: TEXT_SIZE.into(),
            font,
            horizontal_alignment: Horizontal::Left,
            vertical_alignment: Vertical::Center,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}
