//! The rounded card each region of the shell sits in, inset on the frame.

use iced::mouse;
use iced::widget::{canvas, container, stack};
use iced::{Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::message::Message;
use crate::theme as th;

pub const RADIUS: f32 = 10.0;

/// `content` on the panel shade with rounded corners and a hairline edge,
/// filling the space it is given unless sized otherwise.
pub fn shell_card<'a>(content: impl Into<Element<'a, Message>>) -> ShellCard<'a> {
    ShellCard {
        content: content.into(),
        width: Length::Fill,
        height: Length::Fill,
    }
}

pub struct ShellCard<'a> {
    content: Element<'a, Message>,
    width: Length,
    height: Length,
}

impl ShellCard<'_> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<'a> From<ShellCard<'a>> for Element<'a, Message> {
    fn from(card: ShellCard<'a>) -> Self {
        stack![
            container(card.content)
                .width(card.width)
                .height(card.height)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_surface().into()),
                    border: iced::Border {
                        radius: RADIUS.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            canvas(CardFrame).width(Length::Fill).height(Length::Fill),
        ]
        .into()
    }
}

/// Paints the frame shade outside the card's rounded corners and its
/// hairline edge over whatever the content drew there. Content is not
/// clipped to the rounded shape, so lanes and rulers that reach the card's
/// edge would otherwise show square corners.
struct CardFrame;

impl canvas::Program<Message> for CardFrame {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let r = RADIUS.min(w / 2.0).min(h / 2.0);
        if r <= 0.0 {
            return vec![];
        }
        let corners = [
            (Point::ORIGIN, Point::new(r, 0.0), Point::new(0.0, r)),
            (Point::new(w, 0.0), Point::new(w, r), Point::new(w - r, 0.0)),
            (Point::new(w, h), Point::new(w - r, h), Point::new(w, h - r)),
            (Point::new(0.0, h), Point::new(0.0, h - r), Point::new(r, h)),
        ];
        for (corner, from, to) in corners {
            let outside = canvas::Path::new(|path| {
                path.move_to(corner);
                path.line_to(from);
                path.arc_to(corner, to, r);
                path.close();
            });
            frame.fill(&outside, th::display_bg());
        }
        let edge = canvas::Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(w - 1.0, h - 1.0),
            (r - 0.5).into(),
        );
        frame.stroke(
            &edge,
            canvas::Stroke::default()
                .with_color(th::hairline(th::display_bg()))
                .with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}
