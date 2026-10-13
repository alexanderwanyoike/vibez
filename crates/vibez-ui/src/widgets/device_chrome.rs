//! Chrome shared by every device card in the device chain.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};
use vibez_plugin_host::gui::PluginGuiKey;

use crate::icons;
use crate::message::Message;
use crate::theme as th;

/// Card width for hosted CLAP and VST3 devices, instrument or effect.
pub const PLUGIN_CARD_W: f32 = 196.0;

/// Average advance of the 11 px title font. Text shaping cannot be
/// queried while building the view, so names are fitted by estimate.
const TITLE_CHAR_W: f32 = 6.2;

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

/// Device name for a title bar, shortened with an ellipsis to fit
/// `width` so a long plugin name never wraps and grows the title bar.
pub fn title_name<'a>(name: &str, color: iced::Color, width: f32) -> Element<'a, Message> {
    let max_chars = ((width / TITLE_CHAR_W).floor() as usize).max(3);
    let shown = if name.chars().count() > max_chars {
        let mut short: String = name.chars().take(max_chars - 1).collect();
        short.push('\u{2026}');
        short
    } else {
        name.to_string()
    };
    container(
        text(shown)
            .size(11)
            .color(color)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true)
    .into()
}

/// Body of a hosted plugin's card. The plugin draws its own controls, so
/// the card names the format and opens the plugin's editor.
pub fn plugin_face<'a>(
    format: Option<&str>,
    gui_key: Option<PluginGuiKey>,
) -> Element<'a, Message> {
    let format = format.map_or_else(|| "Plugin".to_string(), |format| format.to_uppercase());
    container(
        column![
            section_label(format!("{format} plugin")),
            editor_button(gui_key),
        ]
        .spacing(6),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .height(Length::Fixed(th::DEVICE_BODY_H))
    .into()
}

fn editor_button<'a>(gui_key: Option<PluginGuiKey>) -> Element<'a, Message> {
    let has_editor = gui_key.is_some();
    let label = if has_editor {
        "Open editor"
    } else {
        "No editor"
    };
    button(
        row![
            icons::icon(icons::SLIDERS_VERTICAL).size(11),
            text(label).size(10),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .on_press_maybe(gui_key.map(Message::OpenPluginGui))
    .width(Length::Fill)
    .padding([6, 8])
    .style(move |_theme: &Theme, status| {
        let hovered =
            has_editor && matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(th::bg_dark().into()),
            text_color: if !has_editor {
                th::text_muted()
            } else if hovered {
                th::accent()
            } else {
                th::text()
            },
            border: iced::Border {
                color: if hovered {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

/// Card for a hosted CLAP or VST3 instrument, matching hosted effects.
pub fn plugin_instrument_card<'a>(
    track_id: vibez_core::id::TrackId,
    name: &str,
    format: Option<&str>,
    has_editor: bool,
    latency: String,
    track_color: iced::Color,
) -> Element<'a, Message> {
    let title = title_bar(
        row![
            text("\u{25CF}").size(9).color(track_color),
            title_name(name, th::text(), PLUGIN_CARD_W - 50.0),
            icon_button(
                icons::X,
                th::text_dim(),
                th::danger(),
                Message::remove_track_instrument(track_id),
            ),
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center),
    );
    card(
        column![
            title,
            container(text(latency).size(9).color(th::text_dim())).padding([2, 6]),
            plugin_face(
                format,
                has_editor.then_some(PluginGuiKey::Instrument { track_id }),
            ),
        ]
        .width(Length::Fixed(PLUGIN_CARD_W)),
    )
}
