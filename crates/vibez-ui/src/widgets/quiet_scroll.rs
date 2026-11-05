//! A slim scrollbar that stays out of sight until the pointer is over the
//! area it scrolls.

use iced::widget::scrollable::{Rail, Scrollbar, Scroller, Status, Style};
use iced::{Border, Theme};

use crate::theme as th;

const THICKNESS: f32 = 4.0;

/// The bar itself, floating over the content so showing it moves nothing.
pub fn scrollbar() -> Scrollbar {
    Scrollbar::new().width(THICKNESS).scroller_width(THICKNESS)
}

pub fn style(_theme: &Theme, _status: Status) -> Style {
    let rail = Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller {
            color: th::with_alpha(th::text_muted(), 0.5),
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
    use iced::Color;

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
