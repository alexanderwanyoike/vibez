//! The outline that marks where a Browser drop will land.

use iced::widget::{container, Space};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::widgets::flat_stack::flat_stack;

const OUTLINE_W: f32 = 2.0;

/// `content` with the accent outline drawn over it while `active`. The
/// outline layer stays when inactive, so a hover never rebuilds the
/// content's widget state, and it sits on top so cards cannot paint over it.
pub fn drop_highlight<'a>(content: Element<'a, Message>, active: bool) -> Element<'a, Message> {
    let size = content.as_widget().size_hint();
    let outline = container(Space::new(Length::Fill, Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            border: iced::Border {
                color: if active {
                    th::accent()
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if active { OUTLINE_W } else { 0.0 },
                radius: 4.0.into(),
            },
            ..Default::default()
        });
    flat_stack([content, outline.into()])
        .width(size.width)
        .height(size.height)
        .into()
}
