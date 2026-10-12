//! Chrome shared by every device card in the device chain.

use iced::widget::{button, column, container, text};
use iced::{Element, Length, Theme};
use vibez_plugin_host::gui::PluginGuiKey;

use crate::icons;
use crate::message::Message;
use crate::theme as th;

/// Card width for hosted CLAP and VST3 devices, instrument or effect.
pub const PLUGIN_CARD_W: f32 = 172.0;

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

/// Device name for a title bar. Long plugin names clip rather than wrap
/// so every title bar keeps the same height.
pub fn title_name<'a>(name: &'a str, color: iced::Color) -> Element<'a, Message> {
    container(
        text(name)
            .size(11)
            .color(color)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true)
    .into()
}

/// Body of a hosted plugin's card. The plugin owns its controls, so the
/// card offers its editor and names its format instead of drawing knobs.
pub fn plugin_face<'a>(
    format: Option<&str>,
    gui_key: Option<PluginGuiKey>,
) -> Element<'a, Message> {
    let has_editor = gui_key.is_some();
    let label = if has_editor {
        "Open editor"
    } else {
        "No editor"
    };
    let tile = button(
        column![
            icons::icon(icons::SLIDERS_VERTICAL).size(20),
            text(label).size(10),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Center),
    )
    .on_press_maybe(gui_key.map(Message::OpenPluginGui))
    .width(Length::Fixed(PLUGIN_CARD_W - 40.0))
    .padding([16, 10])
    .style(move |_theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let text_color = if !has_editor {
            th::text_muted()
        } else if hovered {
            th::accent()
        } else {
            th::text_dim()
        };
        button::Style {
            background: Some(
                if hovered {
                    th::bg_hover()
                } else {
                    th::bg_dark()
                }
                .into(),
            ),
            text_color,
            border: iced::Border {
                color: if hovered {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    });
    let badge = text(
        format
            .map(str::to_uppercase)
            .unwrap_or_else(|| "PLUGIN".to_string()),
    )
    .size(8)
    .color(th::text_muted());
    container(
        column![tile, badge]
            .spacing(10)
            .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(th::DEVICE_BODY_H))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center)
    .into()
}
