//! Shared preset selection and file actions for native device panels.

use iced::widget::{button, pick_list, row, text, tooltip};
use iced::{Color, Element, Length, Theme};
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::domains::device_presets::presentation;
use crate::domains::device_presets::DevicePresetMsg;
use crate::message::Message;
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

/// The label and four icon buttons with their spacing; the selector gets the rest.
const FIXED_W: f32 = 140.0;

pub(crate) fn view<'a>(
    track_id: TrackId,
    device: impl Into<DeviceParams<'a>>,
    remembered: Option<&DevicePreset>,
    accent: Color,
    available_width: f32,
) -> Element<'a, Message> {
    let device = device.into();
    let presentation = presentation(device, remembered);
    let slot = device.slot;
    let previous = presentation
        .previous
        .map(|preset| Message::ApplyDevicePreset(track_id, slot, preset));
    let next = presentation
        .next
        .map(|preset| Message::ApplyDevicePreset(track_id, slot, preset));
    let choice = presentation.selected.map(|preset| PresetChoice {
        modified: presentation.modified,
        preset,
    });
    let options = presentation
        .options
        .into_iter()
        .map(|preset| PresetChoice {
            preset,
            modified: false,
        })
        .collect::<Vec<_>>();
    let selector = pick_list(options, choice, move |choice| {
        Message::ApplyDevicePreset(track_id, slot, choice.preset)
    })
    .placeholder("Custom")
    .width(Length::Fixed(
        (available_width - FIXED_W).clamp(72.0, 220.0),
    ))
    .padding([2, 5])
    .text_size(11)
    .style(selector_style(accent))
    .menu_style(menu_style(accent));
    let mut selection = row![text("Preset").size(10).color(th::text_dim()), selector,]
        .spacing(3)
        .align_y(iced::Alignment::Center);
    selection = selection
        .push(preset_button(
            crate::icons::CHEVRON_LEFT,
            "Previous preset",
            previous,
            accent,
        ))
        .push(preset_button(
            crate::icons::CHEVRON_RIGHT,
            "Next preset",
            next,
            accent,
        ));
    let files = row![
        preset_button(
            crate::icons::FOLDER_OPEN,
            "Load preset file",
            Some(Message::DevicePreset(DevicePresetMsg::Load(track_id, slot))),
            accent
        ),
        preset_button(
            crate::icons::SAVE,
            "Save preset file",
            Some(Message::DevicePreset(DevicePresetMsg::Save(track_id, slot))),
            accent
        ),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center);
    row![selection, files]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
}

fn preset_button<'a>(
    icon: char,
    help: &'static str,
    message: Option<Message>,
    accent: Color,
) -> Element<'a, Message> {
    let control = button(crate::icons::icon(icon).size(11))
        .on_press_maybe(message)
        .padding([3, 6])
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
        });
    tooltip(control, text(help).size(9), tooltip::Position::Top).into()
}

fn selector_style(accent: Color) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_theme: &Theme, status| pick_list::Style {
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
    }
}

fn menu_style(accent: Color) -> impl Fn(&Theme) -> iced::widget::overlay::menu::Style {
    move |_theme: &Theme| iced::widget::overlay::menu::Style {
        background: th::bg_surface().into(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: th::text(),
        selected_text_color: accent,
        selected_background: th::bg_hover().into(),
    }
}
