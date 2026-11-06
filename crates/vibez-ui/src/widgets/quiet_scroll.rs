//! A slim scrollbar that stays out of sight until the pointer is over the
//! area it scrolls.

use iced::widget::scrollable::{Direction, Rail, Scrollable, Scrollbar, Scroller, Status, Style};
use iced::{Border, Color, Element, Theme};

use crate::message::Message;
use crate::theme as th;

pub const THICKNESS: f32 = 4.0;

/// The bar itself, floating over the content so showing it moves nothing.
pub fn scrollbar() -> Scrollbar {
    Scrollbar::new().width(THICKNESS).scroller_width(THICKNESS)
}

/// `content` scrolling up and down behind the quiet bar.
pub fn vertical<'a>(content: impl Into<Element<'a, Message>>) -> Scrollable<'a, Message> {
    scroll(content, Direction::Vertical(scrollbar()))
}

/// `content` scrolling sideways behind the quiet bar.
pub fn horizontal<'a>(content: impl Into<Element<'a, Message>>) -> Scrollable<'a, Message> {
    scroll(content, Direction::Horizontal(scrollbar()))
}

/// `content` scrolling in `direction`, for bars that keep room of their own
/// or scroll both ways. Built with its direction because the default
/// vertical check rejects full-height content before a later `.direction`
/// could apply.
pub fn scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    direction: Direction,
) -> Scrollable<'a, Message> {
    Scrollable::with_direction(content, direction).style(style)
}

pub fn style(_theme: &Theme, status: Status) -> Style {
    let shown = !matches!(status, Status::Active);
    let rail = Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller {
            color: if shown {
                th::with_alpha(th::text_muted(), 0.5)
            } else {
                Color::TRANSPARENT
            },
            border: Border {
                radius: (THICKNESS / 2.0).into(),
                ..Default::default()
            },
        },
    };
    Style {
        container: Default::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scroller_shows_only_while_the_pointer_is_over_the_area_or_dragging() {
        let scroller = |status| style(&Theme::Dark, status).horizontal_rail.scroller.color;
        assert_eq!(scroller(Status::Active), Color::TRANSPARENT);
        let hovered = Status::Hovered {
            is_horizontal_scrollbar_hovered: false,
            is_vertical_scrollbar_hovered: false,
        };
        let dragged = Status::Dragged {
            is_horizontal_scrollbar_dragged: true,
            is_vertical_scrollbar_dragged: false,
        };
        assert_ne!(scroller(hovered), Color::TRANSPARENT);
        assert_ne!(scroller(dragged), Color::TRANSPARENT);
    }
}
