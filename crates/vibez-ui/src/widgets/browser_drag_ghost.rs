//! The pill by the cursor that names the sample a Browser drag carries.

use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::canvas;
use iced::{Font, Point, Rectangle, Renderer, Size, Theme};

use crate::message::Message;
use crate::theme;
use crate::widgets::device_controls::text_width;

const HEIGHT: f32 = 24.0;
const PAD_X: f32 = 10.0;
const DOT_R: f32 = 3.0;
const DOT_GAP: f32 = 7.0;
const TITLE_SIZE: f32 = 12.0;
const DETAIL_SIZE: f32 = 11.0;
const SEPARATOR: &str = "  ·  ";
const CURSOR_OFFSET: f32 = 14.0;
const EDGE: f32 = 8.0;
/// Keeps the pill compact however long the sample's name is.
const MAX_W: f32 = 360.0;
/// The sample's name gives way first, so an outcome always reads in full.
const TITLE_MIN_W: f32 = 60.0;

#[derive(Debug, Clone)]
pub struct BrowserDragGhost {
    pub cursor: Point,
    /// The sample being carried, its mode and length.
    pub title: String,
    /// The outcome, where no drop zone already names it.
    pub detail: Option<String>,
    pub valid: Option<bool>,
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
        let detail = self.detail.as_ref().map(|detail| {
            let separator_w = text_width(SEPARATOR, DETAIL_SIZE, font);
            (detail, separator_w, text_width(detail, DETAIL_SIZE, font))
        });
        let detail_w = detail.map_or(0.0, |(_, separator_w, detail_w)| separator_w + detail_w);
        let fixed = PAD_X * 2.0 + DOT_R * 2.0 + DOT_GAP + detail_w;
        let room = (MAX_W.min(bounds.width - EDGE * 2.0) - fixed).max(TITLE_MIN_W);
        let title = crate::typography::fit_text_middle(&self.title, room, TITLE_SIZE, font);
        let title_w = text_width(&title, TITLE_SIZE, font);
        let width = (fixed + title_w).round();

        let x = (self.cursor.x + CURSOR_OFFSET)
            .min((bounds.width - width - EDGE).max(EDGE))
            .round();
        let y = (self.cursor.y + CURSOR_OFFSET)
            .min((bounds.height - HEIGHT - EDGE).max(EDGE))
            .round();
        let pill = canvas::Path::rounded_rectangle(
            Point::new(x + 0.5, y + 0.5),
            Size::new(width - 1.0, HEIGHT - 1.0),
            (HEIGHT / 2.0).into(),
        );
        frame.fill(&pill, theme::bg_elevated());
        frame.stroke(
            &pill,
            canvas::Stroke::default()
                .with_color(theme::border_light())
                .with_width(1.0),
        );

        let mid = y + HEIGHT / 2.0;
        frame.fill(
            &canvas::Path::circle(Point::new(x + PAD_X + DOT_R, mid), DOT_R),
            dot_color,
        );
        let mut pen = x + PAD_X + DOT_R * 2.0 + DOT_GAP;
        let mut put = |content: String, size: f32, color, advance: f32| {
            frame.fill_text(canvas::Text {
                content,
                position: Point::new(pen, mid),
                color,
                size: size.into(),
                font,
                horizontal_alignment: Horizontal::Left,
                vertical_alignment: Vertical::Center,
                ..Default::default()
            });
            pen += advance;
        };
        put(title, TITLE_SIZE, theme::text(), title_w);
        if let Some((detail, separator_w, detail_w)) = detail {
            put(
                SEPARATOR.into(),
                DETAIL_SIZE,
                theme::text_muted(),
                separator_w,
            );
            put(detail.clone(), DETAIL_SIZE, theme::text_dim(), detail_w);
        }
        vec![frame.into_geometry()]
    }
}
