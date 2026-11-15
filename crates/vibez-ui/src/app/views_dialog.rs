//! The card, backdrop and choice button a modal choice dialog is made of.

use iced::widget::{button, center, column, container, mouse_area, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

/// One option of a dialog: its name, a line about it, and what pressing it
/// sends.
pub(super) fn dialog_choice(
    label: String,
    detail: String,
    selected: bool,
    message: Message,
) -> Element<'static, Message> {
    button(
        column![
            text(label)
                .size(13)
                .color(if selected { th::accent() } else { th::text() }),
            text(detail).size(10).color(th::text_dim())
        ]
        .spacing(3),
    )
    .on_press(message)
    .padding([9, 11])
    .width(Length::Fill)
    .style(move |_theme: &Theme, status| button::Style {
        background: Some(
            if selected {
                th::accent_dim()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: if selected { th::accent() } else { th::border() },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// `card` centred over the dimmed window; a press outside it sends
/// `dismiss`.
pub(super) fn dialog_overlay<'a>(
    card: impl Into<Element<'a, Message>>,
    width: f32,
    dismiss: Message,
) -> Element<'a, Message> {
    let card = container(card)
        .width(Length::Fixed(width))
        .padding(18)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });
    mouse_area(
        center(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..Default::default()
            }),
    )
    .on_press(dismiss)
    .into()
}
