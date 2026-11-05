//! The joined group of segments the shell's switches and grouped controls
//! sit in, on a small panel-shaded card on the frame.

use iced::widget::{button, container};
use iced::{Border, Color, Theme};

use crate::theme as th;

pub const GROUP_INSET: f32 = 2.0;
const GROUP_RADIUS: f32 = 8.0;
pub const SEGMENT_RADIUS: f32 = 6.0;

/// A joined group of segments on a small panel-shaded card.
pub fn group(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: Border {
            color: th::hairline(th::display_bg()),
            width: 1.0,
            radius: GROUP_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// A segment inside a group, raised onto the control shade while on and
/// showing `lit`, its colour then.
pub fn segment(lit: Option<Color>) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = match lit {
            Some(_) => Some(th::control().into()),
            None if hovered => Some(th::bg_hover().into()),
            None => None,
        };
        button::Style {
            background,
            text_color: lit.unwrap_or(if hovered { th::text() } else { th::text_dim() }),
            border: Border {
                radius: SEGMENT_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
