//! Native numeric fields, selectors and disclosure controls shared by device bodies.

use iced::widget::{button, canvas, column, pick_list, row, text, text_input, tooltip};
use iced::{Color, Element, Length, Theme};
use vibez_core::id::TrackId;

use crate::domains::device_controls::{
    presented_value, DeviceControlMsg, DeviceControlState, ParameterPresentation,
};
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
    parameter_content(
        track, effect, state, index, accent, knob_size, heading, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn parameter_with_presentation<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
    knob_size: Option<f32>,
    presentation: &ParameterPresentation,
) -> Element<'a, Message> {
    parameter_content(
        track,
        effect,
        state,
        index,
        accent,
        knob_size.filter(|_| presentation.is_editable()),
        section_label(presentation.name),
        Some(presentation),
    )
}

#[allow(clippy::too_many_arguments)]
fn parameter_content<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
    knob_size: Option<f32>,
    heading: Element<'a, Message>,
    presentation: Option<&ParameterPresentation>,
) -> Element<'a, Message> {
    let descriptor = &crate::domains::device_controls::presentation_descriptor(effect, index);
    let value = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(descriptor.default);
    let field = value_field_presented(track, effect, state, index, accent, presentation);
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
        if crate::domains::bitcrush::is_native_bitcrush(effect) {
            use crate::widgets::effect_knob::KnobTarget;
            use vibez_dsp::bitcrush::param;
            knob.target = match index {
                param::BITS => KnobTarget::BitcrushBits(effect.id),
                param::SR_REDUCE => KnobTarget::BitcrushRate(effect.id),
                _ => knob.target,
            };
        }
        let knob: Element<'a, Message> = canvas(knob).width(size).height(size).into();
        let knob = match presentation.and_then(|value| value.explanation.as_ref()) {
            Some(explanation) => tooltip(
                knob,
                text(explanation.clone()).size(9).width(300.0),
                tooltip::Position::Top,
            )
            .into(),
            None => knob,
        };
        content = content.push(knob);
    }
    content
        .push(
            row![
                field,
                text(presentation.map_or(descriptor.unit, |value| value.unit))
                    .size(8)
                    .color(th::text_dim())
            ]
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
    value_field_presented(track, effect, state, index, accent, None)
}

fn value_field_presented<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    index: usize,
    accent: Color,
    presentation: Option<&ParameterPresentation>,
) -> Element<'a, Message> {
    let descriptor = &crate::domains::device_controls::presentation_descriptor(effect, index);
    let value = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(descriptor.default);
    let editable = presentation.is_none_or(ParameterPresentation::is_editable);
    let shown = if editable {
        state.entries.get(&index).cloned().unwrap_or_else(|| {
            presentation
                .and_then(|presentation| presentation.value.clone())
                .unwrap_or_else(|| presented_value(effect, index, value))
        })
    } else {
        "N/A".into()
    };
    let mut explanation = if let Some(presentation) = presentation {
        presentation.range.as_ref().map_or_else(
            || format!("{} unavailable until this native device has a valid prepared host sample rate.", presentation.name),
            |(minimum, maximum)| format!(
                "{}: {} to {} {}. Enter applies.",
                presentation.name, minimum, maximum, presentation.unit
            ),
        )
    } else {
        format!(
            "{}: {} to {} {}. Enter applies.",
            descriptor.name,
            presented_value(effect, index, descriptor.min),
            presented_value(effect, index, descriptor.max),
            descriptor.unit
        )
    };
    if let Some(detail) = presentation.and_then(|value| value.explanation.as_ref()) {
        explanation.push(' ');
        explanation.push_str(detail);
    }
    let explanation = text(explanation).size(9);
    let explanation = if presentation.is_some_and(|value| value.explanation.is_some()) {
        explanation.width(300.0)
    } else {
        explanation
    };
    let effect_id = effect.id;
    let submit = Message::DeviceControl(DeviceControlMsg::Submit(track, effect_id, index));
    let invalid = state.errors.contains(&index);
    let input = text_input("", &shown)
        .on_input_maybe(editable.then_some(move |text| {
            Message::DeviceControl(DeviceControlMsg::Input(track, effect_id, index, text))
        }))
        .on_submit_maybe(editable.then_some(submit.clone()))
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
        on_blur(
            input,
            editable && state.entries.contains_key(&index),
            submit,
        ),
        explanation,
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
    disclosure_with_label(track, effect, open, "Advanced")
}

pub fn disclosure_with_label<'a>(
    track: TrackId,
    effect: &UiEffect,
    open: bool,
    label: &str,
) -> Element<'a, Message> {
    button(text(format!("{} {label}", if open { "▾" } else { "▸" })).size(10))
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
