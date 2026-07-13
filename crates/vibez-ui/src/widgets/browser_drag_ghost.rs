use iced::mouse;
use iced::widget::canvas;
use iced::{Point, Rectangle, Renderer, Size, Theme};

use crate::message::Message;
use crate::theme;

#[derive(Debug, Clone)]
pub struct BrowserDragGhost {
    pub cursor: Point,
    pub title: String,
    pub detail: String,
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
        let width = 292.0_f32.min((bounds.width - 16.0).max(120.0));
        let height = 46.0;
        let x = (self.cursor.x + 14.0).min((bounds.width - width - 8.0).max(8.0));
        let y = (self.cursor.y + 14.0).min((bounds.height - height - 8.0).max(8.0));
        let color = match self.valid {
            Some(true) => theme::accent(),
            Some(false) => theme::danger(),
            None => theme::text_dim(),
        };

        frame.fill_rectangle(
            Point::new(x, y),
            Size::new(width, height),
            theme::bg_elevated(),
        );
        let outline = canvas::Path::rectangle(Point::new(x, y), Size::new(width, height));
        frame.stroke(
            &outline,
            canvas::Stroke::default().with_color(color).with_width(1.0),
        );
        frame.fill_rectangle(Point::new(x, y), Size::new(3.0, height), color);
        frame.fill_text(canvas::Text {
            content: self.title.clone(),
            position: Point::new(x + 11.0, y + 7.0),
            color: theme::text(),
            size: 12.0.into(),
            ..Default::default()
        });
        frame.fill_text(canvas::Text {
            content: self.detail.clone(),
            position: Point::new(x + 11.0, y + 25.0),
            color,
            size: 10.0.into(),
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}
