//! Native numeric fields, selectors and disclosure controls shared by device bodies.

use iced::widget::{button, canvas, column, pick_list, row, text, text_input, tooltip};
use iced::{Color, Element, Length, Theme};
use vibez_core::id::TrackId;

use crate::domains::device_controls::{display_value, DeviceControlMsg, DeviceControlState};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::{
    device_chrome::section_label, effect_knob::EffectKnobWidget, on_blur::on_blur,
};

pub fn parameter<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
    knob_size: Option<f32>,
) -> Element<'a, Message> {
    let heading =
        section_label(crate::domains::device_controls::presentation_descriptor(effect, index).name);
    parameter_with_heading(track, effect, state, index, accent, knob_size, heading)
}

#[allow(clippy::too_many_arguments)]
pub fn parameter_with_heading<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
    knob_size: Option<f32>,
    heading: Element<'a, Message>,
) -> Element<'a, Message> {
    let descriptor = &crate::domains::device_controls::presentation_descriptor(effect, index);
    let value = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(descriptor.default);
    let field = value_field(track, effect, state, index, accent);
    let mut content = column![heading].spacing(3).align_x(iced::Alignment::Center);
    if let Some(size) = knob_size {
        let mut knob = EffectKnobWidget::new(
            track,
            effect.id,
            index,
            value,
            descriptor.min,
            descriptor.max,
            descriptor.default,
            accent,
        );
        if descriptor.unit == "Hz" {
            knob.target = crate::widgets::effect_knob::KnobTarget::LogEffect(effect.id);
        }
        content = content.push(canvas(knob).width(size).height(size));
    }
    content
        .push(
            row![field, text(descriptor.unit).size(8).color(th::text_dim())]
                .spacing(2)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .into()
}

pub fn value_field<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
) -> Element<'a, Message> {
    let descriptor = &crate::domains::device_controls::presentation_descriptor(effect, index);
    let value = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(descriptor.default);
    let shown = state
        .entries
        .get(&index)
        .cloned()
        .unwrap_or_else(|| display_value(value, descriptor));
    let effect_id = effect.id;
    let submit = Message::DeviceControl(DeviceControlMsg::Submit(track, effect_id, index));
    let invalid = state.errors.contains(&index);
    let input = text_input("", &shown)
        .on_input(move |text| {
            Message::DeviceControl(DeviceControlMsg::Input(track, effect_id, index, text))
        })
        .on_submit(submit.clone())
        .size(10)
        .padding([1, 3])
        .width(Length::Fill)
        .font(crate::typography::PERFORM_TECH)
        .style(move |_, status| text_input::Style {
            background: th::bg_dark().into(),
            border: iced::Border {
                color: if invalid {
                    th::danger()
                } else if matches!(status, text_input::Status::Focused) {
                    accent
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            icon: th::text_dim(),
            placeholder: th::text_muted(),
            value: th::text(),
            selection: th::with_alpha(accent, 0.3),
        });
    tooltip(
        on_blur(input, state.entries.contains_key(&index), submit),
        text(format!(
            "{}: {} to {} {}. Enter applies.",
            descriptor.name,
            display_value(descriptor.min, descriptor),
            display_value(descriptor.max, descriptor),
            descriptor.unit
        ))
        .size(9),
        tooltip::Position::Top,
    )
    .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Choice {
    index: usize,
    label: &'static str,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

pub fn selector<'a>(
    track: TrackId,
    effect: &UiEffect,
    index: usize,
    label: &'static str,
    options: &'static [&'static str],
    accent: Color,
) -> Element<'a, Message> {
    let choices: Vec<_> = options
        .iter()
        .enumerate()
        .map(|(index, label)| Choice { index, label })
        .collect();
    let selected = choices
        .get(
            effect
                .params
                .get(index)
                .copied()
                .unwrap_or(effect.descriptors[index].default)
                .round() as usize,
        )
        .copied();
    let id = effect.id;
    let control = pick_list(choices, selected, move |choice| {
        Message::set_effect_param(track, id, index, choice.index as f32)
    })
    .text_size(10)
    .padding([1, 3])
    .width(Length::Fill)
    .style(move |_, _| pick_list::Style {
        text_color: accent,
        placeholder_color: th::text_dim(),
        handle_color: th::text_dim(),
        background: th::bg_dark().into(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 2.0.into(),
        },
    })
    .menu_style(move |_| iced::widget::overlay::menu::Style {
        background: th::bg_surface().into(),
        text_color: th::text(),
        selected_text_color: accent,
        selected_background: th::bg_hover().into(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 2.0.into(),
        },
    });
    column![section_label(label), control]
        .spacing(3)
        .width(Length::Fill)
        .into()
}

pub fn disclosure<'a>(track: TrackId, effect: &UiEffect, open: bool) -> Element<'a, Message> {
    button(text(if open { "▾ Advanced" } else { "▸ Advanced" }).size(10))
        .on_press(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect.id,
        )))
        .padding([2, 4])
        .style(|_: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| th::bg_hover().into()),
            text_color: th::text(),
            ..Default::default()
        })
        .into()
}
