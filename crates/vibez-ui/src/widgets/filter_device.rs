//! Native Filter controls and response placed in measured, non-scrolling regions.

use crate::domains::{
    device_controls::{DeviceControlMsg, DeviceControlState},
    filter_layout::{self, Arrangement, Region, Tier},
};
use crate::widgets::{
    device_chrome::section_label,
    device_controls::{disclosure, value_field},
    effect_knob::{EffectKnobWidget, KnobTarget},
    filter_response::FilterResponse,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{message::Message, state::UiEffect, theme as th};
use iced::{
    widget::{button, canvas, column, container, pick_list, row, text, tooltip, Space, Stack},
    Alignment, Color, Element, Length, Padding, Size,
};
use vibez_core::id::TrackId;
use vibez_dsp::filter::param;
use vibez_engine::metering::MeterValues;

#[path = "filter_modulation.rs"]
mod modulation;

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 620.0;
pub const MAX_W: f32 = 900.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - if size.height < 114.0 { 106.0 } else { 16.0 }
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    preset: Element<'a, Message>,
    output: MeterValues,
    sample_rate: f32,
    size: Size,
) -> Element<'a, Message> {
    let layout = filter_layout::arrange(size.width, size.height, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let toolbar: Element<'a, Message> = if size.height < 114.0 {
        row![
            preset,
            Space::with_width(Length::Fill),
            disclosure(track, effect, state.advanced)
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        preset
    };
    let mut body = Stack::new()
        .push(Space::new(size.width, size.height))
        .push(place(toolbar, layout.toolbar))
        .push(place(
            primary(track, effect, state, accent, layout),
            layout.primary,
        ));
    if let Some(region) = layout.tabs {
        let tab = |label, selected| {
            flat_button(
                label,
                accent,
                selected,
                (!selected).then_some(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
                    track, effect.id,
                ))),
            )
        };
        body = body.push(place(
            row![
                tab("Filter", !state.advanced),
                tab("Modulation", state.advanced)
            ]
            .spacing(6)
            .into(),
            region,
        ));
    }
    if let Some(region) = layout.response {
        let plot = canvas(FilterResponse {
            track,
            effect,
            sample_rate,
            accent,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let response: Element<'a, Message> = if region.height >= 54.0 {
            column![
                section_label("Linear response · drive and modulation excluded"),
                plot
            ]
            .spacing(3)
            .height(Length::Fill)
            .into()
        } else {
            tooltip(
                plot,
                text("Linear response · drive and modulation excluded").size(9),
                tooltip::Position::Top,
            )
            .into()
        };
        body = body.push(place(response, region));
    }
    if let Some(region) = layout.meter {
        body = body.push(place(output_meter(output, region.height), region));
    }
    if let Some(region) = layout.modes {
        body = body.push(place(
            modes(track, effect, accent, layout.vertical_modes, region),
            region,
        ));
    }
    if let Some(region) = layout.modulation {
        let inline_disclosure = layout.tier == Tier::Expanded && size.height >= 114.0;
        body = body.push(place(
            modulation::view(track, effect, state, accent, region, inline_disclosure),
            region,
        ));
    }
    if let Some(region) = layout.disclosure {
        body = body.push(place(disclosure(track, effect, state.advanced), region));
    }
    container(body.width(Length::Fill).height(Length::Fill))
        .clip(true)
        .into()
}

fn place<'a>(content: Element<'a, Message>, region: Region) -> Element<'a, Message> {
    container(
        container(content)
            .width(region.width)
            .height(region.height)
            .clip(true),
    )
    .padding(Padding {
        top: region.y,
        left: region.x,
        right: 0.0,
        bottom: 0.0,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn primary<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    layout: Arrangement,
) -> Element<'a, Message> {
    let mut controls = row![].spacing(6).align_y(Alignment::Center);
    for index in [param::CUTOFF, param::RESONANCE, param::DRIVE, param::MIX] {
        let descriptor = &effect.descriptors[index];
        let field = row![
            container(value_field(track, effect, state, index, accent)).width(54.0),
            text(descriptor.unit).size(8).color(th::text_dim())
        ]
        .spacing(2)
        .align_y(Alignment::Center);
        let mut control = column![].align_x(Alignment::Center).spacing(2);
        if let Some(size) = layout.knob_size {
            let mut knob = EffectKnobWidget::new(
                track,
                effect.id,
                index,
                value(effect, index),
                descriptor.min,
                descriptor.max,
                descriptor.default,
                accent,
            );
            if index == param::CUTOFF {
                knob.target = KnobTarget::LogEffect(effect.id);
            }
            control = control.push(canvas(knob).width(size).height(size));
        }
        controls = controls.push(
            container(control.push(section_label(descriptor.name)).push(field))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        );
    }
    controls.height(Length::Fill).into()
}

fn modes<'a>(
    track: TrackId,
    effect: &UiEffect,
    accent: Color,
    vertical: bool,
    region: Region,
) -> Element<'a, Message> {
    let choice = |index: (usize, f32), label, selected| {
        flat_button(
            label,
            accent,
            selected,
            Some(Message::set_effect_param(
                track, effect.id, index.0, index.1,
            )),
        )
        .height(if region.height < 20.0 { 14.0 } else { 20.0 })
    };
    let selected = value(effect, param::MODE).round() as usize;
    let mut types = row![].spacing(3);
    let mut bank = column![section_label("Type")].spacing(4);
    for (index, label) in ["LP", "HP", "BP", "Notch"].into_iter().enumerate() {
        let button = choice((param::MODE, index as f32), label, index == selected);
        if vertical {
            bank = bank.push(button);
        } else {
            types = types.push(button);
        }
    }
    let slope = row![
        choice(
            (param::SLOPE, 0.0),
            "12 dB",
            value(effect, param::SLOPE) < 0.5
        ),
        choice(
            (param::SLOPE, 1.0),
            "24 dB",
            value(effect, param::SLOPE) >= 0.5
        ),
    ]
    .spacing(3);
    if vertical {
        bank.push(Space::with_height(Length::Fill))
            .push(section_label("Slope"))
            .push(slope)
            .height(Length::Fill)
            .into()
    } else if region.height >= 64.0 {
        column![section_label("Type"), types, section_label("Slope"), slope]
            .spacing(4)
            .into()
    } else {
        row![
            types.width(Length::FillPortion(3)),
            slope.width(Length::FillPortion(2))
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }
}

pub(super) fn flat_button<'a>(
    label: &'static str,
    accent: Color,
    selected: bool,
    message: Option<Message>,
) -> button::Button<'a, Message> {
    button(
        text(label)
            .size(10)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .on_press_maybe(message)
    .padding([0, 6])
    .width(Length::Fill)
    .height(20.0)
    .style(move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| th::bg_hover().into()),
        text_color: if selected { accent } else { th::text_dim() },
        border: iced::Border {
            color: if selected { accent } else { th::border() },
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
}

pub(super) fn value(effect: &UiEffect, index: usize) -> f32 {
    effect
        .params
        .get(index)
        .copied()
        .unwrap_or(effect.descriptors[index].default)
}

fn output_meter<'a>(output: MeterValues, height: f32) -> Element<'a, Message> {
    let mut meter = column![
        section_label("Output"),
        canvas(LevelMeter { levels: output })
            .width(METER_W)
            .height(Length::Fill)
    ]
    .spacing(3)
    .align_x(Alignment::Center)
    .height(Length::Fill);
    if height >= 96.0 {
        for (channel, peak) in [("L", output.peak_l), ("R", output.peak_r)] {
            let db = vibez_core::level::amplitude_to_db(peak);
            let shown = if db.is_finite() {
                format!("{channel} {db:.1}")
            } else {
                format!("{channel} -inf")
            };
            meter = meter.push(
                text(shown)
                    .size(8)
                    .color(th::text())
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
    }
    meter.into()
}

pub(super) fn sync_choice<'a>(
    track: TrackId,
    effect: &UiEffect,
    accent: Color,
) -> Element<'a, Message> {
    const OPTIONS: [&str; 8] = ["Free", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8D"];
    let id = effect.id;
    pick_list(
        OPTIONS.to_vec(),
        Some(OPTIONS[value(effect, param::SYNC).round().clamp(0.0, 7.0) as usize]),
        move |choice| {
            Message::set_effect_param(
                track,
                id,
                param::SYNC,
                OPTIONS
                    .iter()
                    .position(|option| *option == choice)
                    .unwrap_or(0) as f32,
            )
        },
    )
    .text_size(9)
    .padding([0, 3])
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
    })
    .into()
}
