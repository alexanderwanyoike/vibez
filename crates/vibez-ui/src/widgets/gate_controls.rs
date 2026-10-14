//! Native Gate parameter banks and progressive detector details over shared editing controls.

use crate::{
    domains::{
        device_controls::{DeviceControlMsg, DeviceControlState},
        devices::DevicesState,
        gate::{GateMsg, GatePage, GatePanel},
    },
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
    widgets::{
        device_chrome::section_label,
        device_controls::{parameter, parameter_with_heading, selector, value_field},
    },
};
use iced::{
    widget::{button, column, container, pick_list, row, text, tooltip},
    Color, Element, Length,
};
use vibez_core::id::TrackId;
use vibez_dsp::gate::param;
use vibez_engine::metering::MeterValues;

const ASIDE_PAGES: [GatePage; 4] = [
    GatePage::Sound,
    GatePage::Dynamics,
    GatePage::Detector,
    GatePage::Metering,
];

fn aside_page(selected: GatePage) -> GatePage {
    if ASIDE_PAGES.contains(&selected) {
        selected
    } else {
        GatePage::Dynamics
    }
}

fn primary_indices(count: usize) -> &'static [usize] {
    if count == 3 {
        &[param::THRESHOLD, param::HOLD, param::RELEASE]
    } else {
        &[
            param::THRESHOLD,
            param::ATTACK,
            param::HOLD,
            param::RELEASE,
            param::RANGE,
        ]
    }
}

fn value(effect: &UiEffect, index: usize) -> f32 {
    effect
        .params
        .get(index)
        .copied()
        .unwrap_or(effect.descriptors[index].default)
}

pub fn disclosure<'a>(
    track: TrackId,
    effect: &UiEffect,
    open: bool,
    compact: bool,
    accent: Color,
) -> Element<'a, Message> {
    button(text(if compact { "Details" } else { "Advanced" }).size(9))
        .padding([3, 5])
        .on_press(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect.id,
        )))
        .style(move |_, _| button::Style {
            text_color: if open { accent } else { th::text_dim() },
            background: Some(if open { th::bg_hover() } else { th::bg_dark() }.into()),
            border: iced::Border {
                color: if open { accent } else { th::border() },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn toggle<'a>(
    track: TrackId,
    effect: &UiEffect,
    index: usize,
    label: &'static str,
    accent: Color,
) -> Element<'a, Message> {
    let active = value(effect, index) >= 0.5;
    button(text(label).size(9))
        .padding([2, 4])
        .on_press(Message::set_effect_param(
            track,
            effect.id,
            index,
            if active { 0.0 } else { 1.0 },
        ))
        .style(move |_, _| button::Style {
            text_color: if active { accent } else { th::text_dim() },
            background: Some(
                if active {
                    th::bg_hover()
                } else {
                    th::bg_dark()
                }
                .into(),
            ),
            border: iced::Border {
                color: if active { accent } else { th::border() },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn range<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    knobs: Option<f32>,
) -> Element<'a, Message> {
    let enabled = value(effect, param::RANGE_ENABLED) >= 0.5;
    let heading = button(text(if enabled { "RANGE ON" } else { "RANGE OFF" }).size(8))
        .padding(0)
        .on_press(Message::set_effect_param(
            track,
            effect.id,
            param::RANGE_ENABLED,
            if enabled { 0.0 } else { 1.0 },
        ))
        .style(move |_, _| button::Style {
            text_color: if enabled { accent } else { th::text_dim() },
            ..Default::default()
        });
    let heading = tooltip(heading,
        text(format!("Attenuation depth. Click to {} the floor. On: closed floor -{:.1} dB. Off: zero closed target.",
            if enabled { "disable" } else { "enable" }, value(effect, param::RANGE))).size(9),
        tooltip::Position::Top).into();
    parameter_with_heading(track, effect, state, param::RANGE, accent, knobs, heading)
}

pub fn primary<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    knobs: Option<f32>,
    count: usize,
) -> Element<'a, Message> {
    let indices = primary_indices(count);
    let mut controls = row![].spacing(6).align_y(iced::Alignment::Center);
    for &index in indices {
        let control = if index == param::RANGE {
            range(track, effect, state, accent, knobs)
        } else {
            parameter(track, effect, state, index, accent, knobs)
        };
        controls = controls.push(container(control).width(Length::Fill).max_width(94.0));
    }
    controls.into()
}

fn frequency<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    enabled: usize,
    index: usize,
    label: &'static str,
) -> Element<'a, Message> {
    column![
        section_label(label),
        row![
            toggle(track, effect, enabled, "On", accent),
            container(value_field(track, effect, state, index, accent)).width(54.0),
            text("Hz").size(8).color(th::text_dim())
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
    ]
    .spacing(3)
    .width(Length::Fill)
    .into()
}

fn page<'a>(
    track: TrackId,
    effect: &UiEffect,
    selected: GatePage,
    options: Vec<GatePage>,
    accent: Color,
) -> Element<'a, Message> {
    let id = effect.id;
    pick_list(options, Some(selected), move |page| {
        Message::GateControl(GateMsg::Page(track, id, page))
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
    })
    .into()
}

pub fn db(value: f32) -> String {
    let db = vibez_core::level::amplitude_to_db(value);
    if db.is_finite() {
        format!("{db:.1}")
    } else {
        "-inf".into()
    }
}

fn reading<'a>(label: &'static str, shown: String) -> Element<'a, Message> {
    column![
        section_label(label),
        text(shown)
            .size(10)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None)
    ]
    .spacing(3)
    .width(Length::Fill)
    .into()
}

fn readings<'a>(
    panel: &GatePanel,
    input: MeterValues,
    output: MeterValues,
) -> Vec<Element<'a, Message>> {
    let meter = panel.history.meter();
    vec![
        reading("In L", db(input.peak_l)),
        reading("In R", db(input.peak_r)),
        reading("Out L", db(output.peak_l)),
        reading("Out R", db(output.peak_r)),
        reading(
            "GR L",
            meter.map_or("--".into(), |meter| {
                format!("{:.1}", meter.current_reduction_db[0])
            }),
        ),
        reading(
            "GR R",
            meter.map_or("--".into(), |meter| {
                format!("{:.1}", meter.current_reduction_db[1])
            }),
        ),
        reading(
            "Det L",
            meter.map_or("--".into(), |meter| db(meter.detector_peak[0])),
        ),
        reading(
            "Det R",
            meter.map_or("--".into(), |meter| db(meter.detector_peak[1])),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
pub fn advanced<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    panel: &GatePanel,
    accent: Color,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    devices: &DevicesState,
    input: MeterValues,
    output: MeterValues,
    separate_io: bool,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = match panel.page {
        GatePage::Sound => row![
            parameter(track, effect, state, param::ATTACK, accent, None),
            range(track, effect, state, accent, None),
            parameter(track, effect, state, param::MIX, accent, None)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        GatePage::Dynamics => {
            let mut controls = row![
                parameter(track, effect, state, param::HYSTERESIS, accent, None),
                selector(
                    track,
                    effect,
                    param::MODE,
                    "Mode",
                    &["Gate", "Expander"],
                    accent
                )
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            if value(effect, param::MODE) >= 0.5 {
                controls = controls.push(parameter(
                    track,
                    effect,
                    state,
                    param::EXPANSION_RATIO,
                    accent,
                    None,
                ));
            }
            controls.into()
        }
        GatePage::Detector => row![
            toggle(track, effect, param::HP_ENABLED, "HP", accent),
            parameter(track, effect, state, param::HP_FREQUENCY, accent, None),
            toggle(track, effect, param::LP_ENABLED, "LP", accent),
            parameter(track, effect, state, param::LP_FREQUENCY, accent, None),
            toggle(track, effect, param::DETECTOR_LISTEN, "Listen", accent)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        GatePage::Sidechain => {
            let id = effect.id;
            crate::widgets::native_sidechain::view(
                track,
                effect,
                sources,
                buses,
                devices,
                panel.sidechain_input,
                move |input| Message::GateControl(GateMsg::SidechainInput(track, id, input)),
                accent,
                false,
            )
        }
        GatePage::Metering => {
            let mut values = readings(panel, input, output);
            if separate_io {
                values = values.split_off(4);
            }
            row(values).spacing(4).into()
        }
    };
    row![
        column![
            section_label("Details"),
            page(track, effect, panel.page, GatePage::ALL.to_vec(), accent)
        ]
        .spacing(3)
        .width(88.0),
        container(content).width(Length::Fill)
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill)
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn aside<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    panel: &GatePanel,
    accent: Color,
    input: MeterValues,
    output: MeterValues,
) -> Element<'a, Message> {
    let disclosure = disclosure(track, effect, state.advanced, false, accent);
    if !state.advanced {
        return disclosure;
    }
    let selected = aside_page(panel.page);
    let header = row![
        disclosure,
        page(
            track,
            effect,
            selected,
            ASIDE_PAGES.to_vec(),
            accent
        )
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    let controls: Element<'a, Message> = match selected {
        GatePage::Dynamics | GatePage::Sidechain => {
            let mut mode = column![selector(
                track,
                effect,
                param::MODE,
                "Mode",
                &["Gate", "Expander"],
                accent
            )]
            .spacing(4);
            if value(effect, param::MODE) >= 0.5 {
                mode = mode.push(parameter(
                    track,
                    effect,
                    state,
                    param::EXPANSION_RATIO,
                    accent,
                    None,
                ));
            }
            row![
                container(parameter(
                    track,
                    effect,
                    state,
                    param::HYSTERESIS,
                    accent,
                    Some(20.0)
                ))
                .width(60.0),
                container(
                    column![
                        frequency(
                            track,
                            effect,
                            state,
                            accent,
                            param::HP_ENABLED,
                            param::HP_FREQUENCY,
                            "Detector HP"
                        ),
                        toggle(track, effect, param::DETECTOR_LISTEN, "Listen", accent)
                    ]
                    .spacing(4)
                )
                .width(110.0),
                container(mode).width(110.0)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        }
        GatePage::Sound => row![
            parameter(track, effect, state, param::ATTACK, accent, None),
            range(track, effect, state, accent, None),
            parameter(track, effect, state, param::MIX, accent, None)
        ]
        .spacing(6)
        .into(),
        GatePage::Detector => column![
            row![
                frequency(
                    track,
                    effect,
                    state,
                    accent,
                    param::HP_ENABLED,
                    param::HP_FREQUENCY,
                    "Detector HP"
                ),
                frequency(
                    track,
                    effect,
                    state,
                    accent,
                    param::LP_ENABLED,
                    param::LP_FREQUENCY,
                    "Detector LP"
                )
            ]
            .spacing(8),
            toggle(
                track,
                effect,
                param::DETECTOR_LISTEN,
                "Listen to detector",
                accent
            )
        ]
        .spacing(4)
        .into(),
        GatePage::Metering => {
            let mut values = readings(panel, input, output);
            let right = values.split_off(4);
            column![row(values).spacing(4), row(right).spacing(4)]
                .spacing(4)
                .into()
        }
    };
    column![header, controls]
        .spacing(3)
        .width(Length::Fill)
        .into()
}
