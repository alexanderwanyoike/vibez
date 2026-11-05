//! The filled, borderless look buttons and switches share: the control
//! shade at rest, the hover shade under the pointer, solid colour while on.

use iced::widget::button;
use iced::{Border, Color, Theme};

pub const RADIUS: f32 = 4.0;

/// The fill of a control lit in `on`, if it is on.
pub fn fill(_on: Option<Color>, _status: button::Status) -> Color {
    todo!()
}

/// The label colour over [`fill`].
pub fn ink(_on: Option<Color>, _status: button::Status) -> Color {
    todo!()
}

/// A control's style, lit in `on` while it is on.
pub fn style(on: Option<Color>) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| button::Style {
        background: Some(fill(on, status).into()),
        text_color: ink(on, status),
        border: Border {
            radius: RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme as th;

    const LIT: Color = Color::from_rgb(0.9, 0.5, 0.1);

    #[test]
    fn an_idle_control_is_filled_with_the_control_shade_and_has_no_border() {
        let idle = style(None)(&Theme::Dark, button::Status::Active);

        assert_eq!(idle.background, Some(th::control().into()));
        assert_eq!(idle.border.width, 0.0);
        assert_eq!(idle.text_color, th::text_dim());
    }

    #[test]
    fn a_hovered_control_lifts_to_the_hover_shade() {
        let hovered = style(None)(&Theme::Dark, button::Status::Hovered);

        assert_eq!(hovered.background, Some(th::bg_hover().into()));
        assert_eq!(hovered.text_color, th::text());
    }

    #[test]
    fn a_lit_control_is_filled_with_its_colour_under_dark_ink() {
        for status in [button::Status::Active, button::Status::Hovered] {
            let lit = style(Some(LIT))(&Theme::Dark, status);

            assert_eq!(lit.background, Some(LIT.into()), "{status:?}");
            assert_eq!(lit.text_color, th::bg_dark(), "{status:?}");
            assert_eq!(lit.border.width, 0.0);
        }
    }
}
