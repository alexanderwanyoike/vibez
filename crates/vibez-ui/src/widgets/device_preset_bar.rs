//! Shared preset selection and file actions for native device panels.

use iced::widget::{pick_list, row, text};
use iced::{Color, Element, Length, Theme};
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::domains::device_presets::presentation;
use crate::domains::device_presets::DevicePresetMsg;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::control_style;
use crate::widgets::device_chrome::tiny_button;

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

/// The preset selector `selector_width` wide, then previous, next, load
/// and save, as a device card's header carries them.
pub(crate) fn view<'a>(
    track_id: TrackId,
    device: impl Into<DeviceParams<'a>>,
    remembered: Option<&DevicePreset>,
    accent: Color,
    selector_width: f32,
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
    .width(Length::Fixed(selector_width))
    .padding([4, 8])
    .text_size(11)
    .style(control_style::list(accent))
    .menu_style(menu_style(accent));
    row![
        selector,
        tiny_button(
            crate::icons::CHEVRON_LEFT,
            "Previous preset",
            previous,
            None,
            accent
        ),
        tiny_button(
            crate::icons::CHEVRON_RIGHT,
            "Next preset",
            next,
            None,
            accent
        ),
        tiny_button(
            crate::icons::FOLDER_OPEN,
            "Load preset file",
            Some(Message::DevicePreset(DevicePresetMsg::Load(track_id, slot))),
            None,
            accent
        ),
        tiny_button(
            crate::icons::SAVE,
            "Save preset file",
            Some(Message::DevicePreset(DevicePresetMsg::Save(track_id, slot))),
            None,
            accent
        ),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center)
    .into()
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

#[derive(Clone, Debug, PartialEq)]
struct KitChoice {
    index: usize,
    name: &'static str,
}

impl std::fmt::Display for KitChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name)
    }
}

/// A Drum Rack's kit: the factory kits to load, and opening or saving a kit
/// folder. `kit` is the one last loaded or saved.
pub(crate) fn kit_bar<'a>(
    track_id: TrackId,
    kit: Option<&str>,
    accent: Color,
    available_width: f32,
) -> Element<'a, Message> {
    let options: Vec<KitChoice> = crate::domains::drum_kits::FACTORY_KITS
        .iter()
        .enumerate()
        .map(|(index, factory)| KitChoice {
            index,
            name: factory.name,
        })
        .collect();
    let chosen = kit.and_then(|kit| options.iter().find(|option| option.name == kit).cloned());
    let selector = pick_list(options, chosen, move |choice| {
        Message::LoadFactoryDrumKit(track_id, choice.index)
    })
    .placeholder(kit.unwrap_or("Choose a kit").to_string())
    .width(Length::Fixed(
        (available_width - KIT_FIXED_W).clamp(72.0, 220.0),
    ))
    .padding([4, 8])
    .text_size(11)
    .style(control_style::list(accent))
    .menu_style(menu_style(accent));
    row![
        text("Kit").size(10).color(th::text_dim()),
        selector,
        tiny_button(
            crate::icons::FOLDER_OPEN,
            "Open kit folder",
            Some(Message::OpenDrumKit(track_id)),
            None,
            accent
        ),
        tiny_button(
            crate::icons::SAVE,
            "Save kit folder",
            Some(Message::SaveDrumKit(track_id)),
            None,
            accent
        ),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center)
    .into()
}

/// The label and two icon buttons with their spacing.
const KIT_FIXED_W: f32 = 72.0;
