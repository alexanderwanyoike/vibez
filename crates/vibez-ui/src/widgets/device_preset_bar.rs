//! Shared preset selection and file actions for native device panels.

use iced::widget::{button, column, container, pick_list, row, text, tooltip};
use iced::{Color, Element, Length, Theme};
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::TrackId;

use crate::app::device_presets::DevicePresetMsg;
use crate::domains::device_presets::presentation;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;

#[derive(Clone, Debug, PartialEq)]
struct PresetChoice {
    preset: DevicePreset,
    modified: bool,
}

impl std::fmt::Display for PresetChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.preset.name();
        if name.chars().count() > 24 {
            for character in name.chars().take(23) {
                write!(formatter, "{character}")?;
            }
            formatter.write_str("…")?;
        } else {
            formatter.write_str(name)?;
        }
        if self.modified {
            formatter.write_str(" *")?;
        }
        Ok(())
    }
}

pub(crate) fn view<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    remembered: Option<&DevicePreset>,
    accent: Color,
    available_width: f32,
) -> Element<'a, Message> {
    let presentation = presentation(effect, remembered);
    let previous = presentation
        .previous
        .map(|preset| Message::ApplyDevicePreset(track_id, effect.id, preset));
    let next = presentation
        .next
        .map(|preset| Message::ApplyDevicePreset(track_id, effect.id, preset));
    let choice = presentation.selected.map(|preset| PresetChoice {
        modified: presentation.modified,
        preset,
    });
    let hint = choice.as_ref().map_or_else(
        || "Custom settings".to_string(),
        |choice| {
            format!(
                "{}{}",
                choice.preset.name(),
                if choice.modified { " (modified)" } else { "" }
            )
        },
    );
    let options = presentation
        .options
        .into_iter()
        .map(|preset| PresetChoice {
            preset,
            modified: false,
        })
        .collect::<Vec<_>>();
    let effect_id = effect.id;
    let selector = pick_list(options, choice, move |choice| {
        Message::ApplyDevicePreset(track_id, effect_id, choice.preset)
    })
    .placeholder("Custom")
    .width(Length::Fixed((available_width - 95.0).clamp(72.0, 200.0)))
    .padding([3, 5])
    .text_size(12)
    .style(move |_theme: &Theme, status| pick_list::Style {
        text_color: th::text(),
        placeholder_color: th::text_dim(),
        handle_color: th::text_dim(),
        background: th::bg_dark().into(),
        border: iced::Border {
            color: if matches!(
                status,
                pick_list::Status::Opened | pick_list::Status::Hovered
            ) {
                accent
            } else {
                th::border()
            },
            width: 1.0,
            radius: 3.0.into(),
        },
    })
    .menu_style(move |_theme: &Theme| iced::widget::overlay::menu::Style {
        background: th::bg_surface().into(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: th::text(),
        selected_text_color: accent,
        selected_background: th::bg_hover().into(),
    });
    let selection = row![
        text("Preset").size(12).color(th::text_dim()),
        tooltip(selector, text(hint).size(11), tooltip::Position::Top),
        preset_button("‹", previous, accent),
        preset_button("›", next, accent),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center);
    let files = row![
        preset_button(
            "Load",
            Some(Message::DevicePreset(DevicePresetMsg::Load(
                track_id, effect_id
            ))),
            accent
        ),
        preset_button(
            "Save",
            Some(Message::DevicePreset(DevicePresetMsg::Save(
                track_id, effect_id
            ))),
            accent
        ),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center);
    if available_width < 370.0 {
        column![selection, container(files).padding([0, 33])]
            .spacing(3)
            .width(Length::Fill)
            .into()
    } else {
        row![selection, files]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    }
}

fn preset_button<'a>(
    label: &'static str,
    message: Option<Message>,
    accent: Color,
) -> Element<'a, Message> {
    button(text(label).size(12))
        .on_press_maybe(message)
        .padding([3, 5])
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(th::bg_surface().into()),
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                accent
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}
