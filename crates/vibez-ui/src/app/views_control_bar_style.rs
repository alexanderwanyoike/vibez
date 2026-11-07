//! Shared look of the control bar's groups, segments and recessed display.

use iced::widget::{button, container, pick_list, text, text_input, Space};
use iced::{Border, Color, Element, Length, Size, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::typography;

pub(super) use crate::widgets::segment_group::{group, segment, GROUP_INSET, SEGMENT_RADIUS};

pub(super) const BAR_HEIGHT: f32 = 52.0;
pub(super) const DISPLAY_HEIGHT: f32 = 38.0;
const DISPLAY_LABEL_SIZE: f32 = 8.0;

/// The bar sits straight on the frame, like the gaps between the cards.
pub(super) fn bar(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::display_bg().into()),
        ..Default::default()
    }
}

/// A fixed size segment holding centred `content`; `lit` as in [`segment`].
pub(super) fn segment_button<'a>(
    content: impl Into<Element<'a, Message>>,
    size: Size,
    lit: Option<Color>,
    message: Message,
) -> Element<'a, Message> {
    button(
        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(message)
    .width(Length::Fixed(size.width))
    .height(Length::Fixed(size.height))
    .padding(0)
    .style(segment(lit))
    .into()
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

/// The position display, carded like the groups beside it.
pub(super) fn display(theme: &Theme) -> container::Style {
    group(theme)
}

pub(super) fn display_divider<'a>() -> Element<'a, Message> {
    container(rule(
        Length::Fixed(1.0),
        Length::Fill,
        th::hairline(th::bg_surface()),
    ))
    .padding([8, 0])
    .into()
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

/// A solid bar of `colour`, for dividers.
pub(super) fn rule<'a>(width: Length, height: Length, colour: Color) -> Element<'a, Message> {
    container(Space::new(width, height))
        .style(move |_theme: &Theme| container::Style {
            background: Some(colour.into()),
            ..Default::default()
        })
        .into()
}
