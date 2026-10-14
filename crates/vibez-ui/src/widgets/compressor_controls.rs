//! Native Compressor sound and detector pages using shared numeric and selector widgets.

use crate::{
    domains::{
        compressor::{CompressorMsg, CompressorPage, CompressorPanel},
        device_controls::DeviceControlState,
        devices::DevicesState,
    },
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
    widgets::{
        device_chrome::section_label,
        device_controls::{parameter, selector, value_field},
    },
};
use iced::{
    widget::{button, column, container, pick_list, row, text, tooltip},
    Color, Element, Length,
};
use vibez_core::id::{EffectId, TrackId};
use vibez_dsp::compressor::param;
use vibez_engine::metering::MeterValues;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryControls {
    Core,
    Full,
    Expanded,
}

impl PrimaryControls {
    pub fn indices(self) -> &'static [usize] {
        match self {
            Self::Core => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
            ],
            Self::Full | Self::Expanded => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
                param::KNEE,
                param::MAKEUP,
                param::MIX,
            ],
        }
    }
}

pub fn primary<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    knobs: Option<f32>,
    controls: PrimaryControls,
) -> Element<'a, Message> {
    let mut parameters = row![].spacing(6).align_y(iced::Alignment::Center);
    for &index in controls.indices() {
        parameters = parameters.push(parameter(track, effect, state, index, accent, knobs));
        if controls == PrimaryControls::Expanded && index == param::RELEASE {
            parameters = parameters.push(
                container(toggle(track, effect, param::AUTO_RELEASE, "Auto", accent)).width(44.0),
            );
        }
    }
    parameters.width(Length::Fill).into()
}

pub fn strip<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
) -> Element<'a, Message> {
    let frequency = |enabled, index, label| -> Element<'a, Message> {
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
        .into()
    };
    row![
        frequency(param::HP_ENABLED, param::HP_FREQUENCY, "Detector HP"),
        frequency(param::LP_ENABLED, param::LP_FREQUENCY, "Detector LP"),
        container(selector(
            track,
            effect,
            param::DETECTOR_MODE,
            "Detector",
            &["Peak", "RMS"],
            accent
        ))
        .width(82.0),
        toggle(track, effect, param::STEREO_LINK, "Stereo Link", accent),
        toggle(track, effect, param::DETECTOR_LISTEN, "Listen", accent),
        column![
            section_label("Reduction cap"),
            row![
                toggle(track, effect, param::RANGE_ENABLED, "On", accent),
                container(value_field(track, effect, state, param::RANGE, accent)).width(50.0),
                text("dB").size(8).color(th::text_dim())
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center)
        ]
        .spacing(3)
    ]
    .spacing(14)
    .align_y(iced::Alignment::Center)
    .into()
}

fn toggle<'a>(
    track: TrackId,
    effect: &UiEffect,
    index: usize,
    label: &'static str,
    accent: Color,
) -> Element<'a, Message> {
    let active = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(effect.descriptors[index].default)
        >= 0.5;
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
fn reading<'a>(label: &'static str, value: f32, db: bool) -> Element<'a, Message> {
    let value = if db {
        vibez_core::level::amplitude_to_db(value)
    } else {
        value
    };
    let shown = if value.is_finite() {
        format!("{value:.1}")
    } else {
        "-inf".into()
    };
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
fn page_selector<'a>(
    track: TrackId,
    id: EffectId,
    pages: Vec<CompressorPage>,
    selected: CompressorPage,
    accent: Color,
) -> Element<'a, Message> {
    pick_list(pages, Some(selected), move |page| {
        Message::CompressorControl(CompressorMsg::Page(track, id, page))
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

#[allow(clippy::too_many_arguments)]
pub fn advanced<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    panel: &CompressorPanel,
    accent: Color,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    devices: &DevicesState,
    input: MeterValues,
    output: MeterValues,
) -> Element<'a, Message> {
    let page = page_selector(track, effect.id, CompressorPage::ALL.to_vec(), panel.page, accent);
    let page = column![section_label("Details"), page]
        .spacing(3)
        .width(88.0);
    let controls: Element<'a, Message> = match panel.page {
        CompressorPage::Sound => row![
            parameter(track, effect, state, param::KNEE, accent, None),
            parameter(track, effect, state, param::MAKEUP, accent, None),
            parameter(track, effect, state, param::MIX, accent, None)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into(),
        CompressorPage::Dynamics => row![
            selector(
                track,
                effect,
                param::DETECTOR_MODE,
                "Detector",
                &["Peak", "RMS"],
                accent
            ),
            tooltip(
                selector(
                    track,
                    effect,
                    param::AUTO_RELEASE,
                    "Release",
                    &["Manual", "Adaptive"],
                    accent
                ),
                text("Adaptive follows signal activity. Release sets its base time.").size(9),
                tooltip::Position::Top
            ),
            selector(
                track,
                effect,
                param::STEREO_LINK,
                "Stereo",
                &["Unlinked", "Linked"],
                accent
            ),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        CompressorPage::Detector => row![
            toggle(track, effect, param::HP_ENABLED, "HP", accent),
            parameter(track, effect, state, param::HP_FREQUENCY, accent, None),
            toggle(track, effect, param::LP_ENABLED, "LP", accent),
            parameter(track, effect, state, param::LP_FREQUENCY, accent, None),
            toggle(track, effect, param::DETECTOR_LISTEN, "Listen", accent),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        CompressorPage::Range => row![
            toggle(track, effect, param::RANGE_ENABLED, "Limit GR", accent),
            parameter(track, effect, state, param::RANGE, accent, None)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into(),
        CompressorPage::Sidechain => crate::widgets::native_sidechain::view(
            track, effect, sources, buses, devices, panel, accent, false,
        ),
        CompressorPage::Metering => row![
            reading("In L", input.peak_l, true),
            reading("In R", input.peak_r, true),
            reading("Out L", output.peak_l, true),
            reading("Out R", output.peak_r, true),
            reading("GR L", panel.meter.current_reduction_db[0], false),
            reading("GR R", panel.meter.current_reduction_db[1], false),
            reading("Det L", panel.meter.detector_peak[0], true),
            reading("Det R", panel.meter.detector_peak[1], true)
        ]
        .spacing(4)
        .into(),
    };
    row![page, container(controls).width(Length::Fill)]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
}
