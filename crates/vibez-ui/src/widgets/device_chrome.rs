//! Chrome shared by every device card in the device chain.

use iced::widget::{button, container, text};
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

/// Opens a hosted plugin's native editor.
pub fn edit_button<'a>(
    gui_key: vibez_plugin_host::gui::PluginGuiKey,
) -> button::Button<'a, Message> {
    button(text("Edit").size(9).color(th::text_dim()))
        .on_press(Message::OpenPluginGui(gui_key))
        .padding([2, 5])
        .style(|_theme: &Theme, status| {
            let (bg, tc) = match status {
                button::Status::Hovered => (Some(th::bg_hover().into()), th::accent()),
                _ => (None, th::text_dim()),
            };
            button::Style {
                background: bg,
                text_color: tc,
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
}
