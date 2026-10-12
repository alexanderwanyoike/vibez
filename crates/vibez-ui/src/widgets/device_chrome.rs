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

/// Tiny uppercase header above a group of controls inside a device body.
pub fn section_label<'a>(label: impl ToString) -> Element<'a, Message> {
    text(label.to_string().to_uppercase())
        .size(8)
        .color(th::text_muted())
        .into()
}

/// Dark strip across the top of every device card.
pub fn title_bar<'a>(
    content: impl Into<Element<'a, Message>>,
) -> container::Container<'a, Message> {
    container(content)
        .padding([4, 6])
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            ..Default::default()
        })
}

/// Raised, bordered container wrapping a whole device card.
pub fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Small icon-only action in a device title bar.
pub fn icon_button(
    icon_char: char,
    color: iced::Color,
    hover_color: iced::Color,
    msg: Message,
) -> button::Button<'static, Message> {
    button(icons::icon(icon_char).size(12).color(color))
        .on_press(msg)
        .padding([3, 4])
        .style(move |_theme: &Theme, status| {
            let (bg, tc) = match status {
                button::Status::Hovered => (Some(th::bg_hover().into()), hover_color),
                button::Status::Pressed => (Some(th::bg_dark().into()), hover_color),
                _ => (None, color),
            };
            button::Style {
                background: bg,
                text_color: tc,
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
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
