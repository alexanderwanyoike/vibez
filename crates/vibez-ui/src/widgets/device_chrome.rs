//! Chrome shared by every device card in the device chain.

use iced::widget::{container, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

/// Thin vertical rule separating sections inside a device body.
pub fn divider() -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(th::DEVICE_BODY_H - 12.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}
