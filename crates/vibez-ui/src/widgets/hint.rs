//! A control with a short explanation shown below it on hover.

use iced::widget::{container, text, tooltip};
use iced::{Border, Element, Theme};

use crate::message::Message;
use crate::theme as th;

pub(crate) fn with_hint<'a>(
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
