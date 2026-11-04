//! Shared look of the control bar's groups, segments and recessed display.

use iced::widget::{button, container, pick_list, text, text_input, tooltip, Space};
use iced::{Border, Color, Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::typography;

pub(super) const BAR_HEIGHT: f32 = 56.0;
pub(super) const GROUP_INSET: f32 = 2.0;
const GROUP_RADIUS: f32 = 7.0;
const SEGMENT_RADIUS: f32 = 5.0;
/// How far a lit segment's background leans toward its colour.
const LIT_TINT: f32 = 0.2;
pub(super) const DISPLAY_HEIGHT: f32 = 42.0;
const DISPLAY_LABEL_SIZE: f32 = 8.0;

pub(super) fn bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        ..Default::default()
    }
}

/// The hairline under the bar.
pub(super) fn bar_rule<'a>() -> Element<'a, Message> {
    rule(Length::Fill, Length::Fixed(1.0))
}

/// A joined group of segments on one raised strip.
pub(super) fn group(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_elevated().into()),
        border: Border {
            color: th::border(),
            width: 1.0,
            radius: GROUP_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// A segment inside a group; `lit` is the colour it shows while on.
pub(super) fn segment(lit: Option<Color>) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = match lit {
            Some(color) => Some(th::blend(th::bg_elevated(), color, LIT_TINT).into()),
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

/// A pick list that sits flush inside a group.
pub(super) fn group_pick_list(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let highlighted = matches!(
        status,
        pick_list::Status::Hovered | pick_list::Status::Opened
    );
    pick_list::Style {
        text_color: th::text(),
        placeholder_color: th::text_dim(),
        handle_color: if highlighted {
            th::accent()
        } else {
            th::text_dim()
        },
        background: if highlighted {
            th::bg_hover().into()
        } else {
            Color::TRANSPARENT.into()
        },
        border: Border {
            radius: SEGMENT_RADIUS.into(),
            ..Default::default()
        },
    }
}

pub(super) fn group_pick_list_menu(_theme: &Theme) -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        background: th::bg_elevated().into(),
        border: Border {
            color: th::border_light(),
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: th::text(),
        selected_text_color: th::accent(),
        selected_background: th::bg_hover().into(),
    }
}

/// The recessed position display, darker than the bar in a dark theme and
/// lighter in a light one.
pub(super) fn display(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::blend(th::bg_dark(), th::display_bg(), 0.5).into()),
        border: Border {
            color: th::border(),
            width: 1.0,
            radius: GROUP_RADIUS.into(),
        },
        ..Default::default()
    }
}

pub(super) fn display_divider<'a>() -> Element<'a, Message> {
    rule(Length::Fixed(1.0), Length::Fill)
}

/// An editable value in the display, drawn as plain readout text until it
/// has focus.
pub(super) fn display_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused);
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border {
            color: if focused {
                th::accent_dim()
            } else {
                Color::TRANSPARENT
            },
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: th::text_dim(),
        placeholder: th::text_muted(),
        value: if matches!(status, text_input::Status::Disabled) {
            th::text_dim()
        } else {
            th::text()
        },
        selection: th::with_alpha(th::accent(), 0.35),
    }
}

pub(super) fn display_label<'a>(label: &'a str, color: Color) -> iced::widget::Text<'a> {
    text(label)
        .font(typography::PERFORM_LABEL)
        .size(DISPLAY_LABEL_SIZE)
        .color(color)
        .wrapping(text::Wrapping::None)
}

/// A control with a short explanation shown below it on hover.
pub(super) fn with_hint<'a>(
    control: impl Into<Element<'a, Message>>,
    hint: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    let hint = container(text(hint).size(10).color(th::text()))
        .padding([5, 7])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    tooltip(control, hint, tooltip::Position::Bottom)
        .gap(6)
        .padding(0)
        .into()
}

fn rule<'a>(width: Length, height: Length) -> Element<'a, Message> {
    container(Space::new(width, height))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::border().into()),
            ..Default::default()
        })
        .into()
}
