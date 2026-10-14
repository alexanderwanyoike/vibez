//! Composes native Bitcrush core fields, six Details pages and genuine clock/level readings.

use crate::{
    domains::{
        bitcrush::BitcrushMsg,
        bitcrush_layout::{CoreControl, DetailCell, DetailPage, Details},
        device_controls::{DeviceControlState, ParameterPresentation},
    },
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        device_chrome::section_label,
        device_controls::{parameter_with_presentation, selector},
    },
};
use iced::{widget::{column, container, pick_list, row, text, tooltip}, Color, Element, Length};
use vibez_core::{effect::SampleHoldMeter, id::TrackId};
use vibez_dsp::bitcrush::param;
use vibez_engine::metering::MeterValues;

fn parameter<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    index: usize,
    knob: Option<f32>,
    presentations: &[ParameterPresentation],
) -> Element<'a, Message> {
    match index {
        param::ROUNDING => selector(track, effect, index, "Rounding", &["Nearest", "Floor", "Truncate"], accent),
        param::DITHER => selector(track, effect, index, "Dither", &["Off", "TPDF"], accent),
        param::POST_LP_ENABLED => tooltip(
            selector(track, effect, index, "Post LP", &["Off", "On"], accent),
            text("Creative low-pass after acquisition. It cannot remove aliases already folded by sample-and-hold.").size(9),
            tooltip::Position::Top,
        ).into(),
        _ => {
            let Some(presentation) = presentations.get(index) else {
                return text("N/A").size(10).color(th::text_dim()).into();
            };
            let knob = if matches!(index, param::BITS | param::SR_REDUCE) { None } else { knob };
            parameter_with_presentation(track, effect, state, index, accent, knob, presentation)
        }
    }
}

pub fn primary<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    controls: &[CoreControl],
    presentations: &[ParameterPresentation],
) -> Element<'a, Message> {
    let gap = controls.windows(2).next().map_or(6.0, |pair| {
        pair[1].bounds.x - pair[0].bounds.x - pair[0].bounds.width
    });
    row(controls.iter().map(|control| {
        container(parameter(track, effect, state, accent, control.parameter, control.knob_size, presentations))
            .width(control.bounds.width).height(control.bounds.height)
            .align_y(iced::Alignment::End).into()
    }))
    .spacing(gap).into()
}

fn page_selector<'a>(track: TrackId, effect: &UiEffect, page: DetailPage, accent: Color) -> Element<'a, Message> {
    let id = effect.id;
    let pages = pick_list(DetailPage::ALL, Some(page), move |page| {
        Message::BitcrushControl(BitcrushMsg::Page(track, id, page))
    })
    .text_size(10).padding([1, 3]).width(Length::Fill)
    .style(move |_, _| pick_list::Style {
        text_color: accent, placeholder_color: th::text_dim(), handle_color: th::text_dim(),
        background: th::bg_dark().into(),
        border: iced::Border { color: th::border(), width: 1.0, radius: 2.0.into() },
    })
    .menu_style(move |_| iced::widget::overlay::menu::Style {
        background: th::bg_surface().into(), text_color: th::text(), selected_text_color: accent,
        selected_background: th::bg_hover().into(),
        border: iced::Border { color: th::border(), width: 1.0, radius: 2.0.into() },
    });
    column![section_label("Details"), pages].spacing(3).width(88.0).into()
}

pub fn decibels(value: Option<f32>) -> String {
    value.map_or_else(|| "N/A".into(), |value| {
        let db = vibez_core::level::amplitude_to_db(value);
        if db.is_finite() { format!("{db:.1}") } else { "-inf".into() }
    })
}

fn reading<'a>(label: &str, value: Option<f32>, explanation: &'static str) -> Element<'a, Message> {
    tooltip(
        column![section_label(label),
            text(decibels(value)).size(10).font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None).color(th::text())]
            .spacing(3),
        text(explanation).size(9), tooltip::Position::Top,
    ).into()
}

fn clock<'a>(meter: Option<SampleHoldMeter>) -> Element<'a, Message> {
    let status = meter.map_or_else(|| "N/A".into(), |meter| {
        if meter.primary_interval_frames == 0 {
            "N/A".into()
        } else if meter.transition_active {
            format!("Blend {}/{}", meter.primary_interval_frames, meter.secondary_interval_frames)
        } else {
            format!("N{}", meter.primary_interval_frames)
        }
    });
    tooltip(
        column![section_label("Clock"), text(status).size(9)
            .font(crate::typography::PERFORM_TECH).wrapping(iced::widget::text::Wrapping::None)]
            .spacing(3),
        text("Actual most recently acquired interval at packet end, in source frames. Blend lists outgoing/incoming lanes. Historical acquisition ticks may use other intervals; this is not the requested Rate divider.").size(9),
        tooltip::Position::Top,
    ).into()
}

#[allow(clippy::too_many_arguments)]
pub fn details<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    layout: &Details,
    presentations: &[ParameterPresentation],
    meter: Option<SampleHoldMeter>,
    levels: Option<(MeterValues, MeterValues)>,
) -> Element<'a, Message> {
    let cells = row(layout.cells.iter().zip(&layout.cell_bounds).map(|(cell, bounds)| {
        let content = match *cell {
            DetailCell::Parameter(index) => parameter(track, effect, state, accent, index, None, presentations),
            DetailCell::ClockStatus => clock(meter),
            DetailCell::InputPeakLeft => reading("IL peak", levels.map(|v| v.0.peak_l), "Input left peak, dBFS."),
            DetailCell::InputPeakRight => reading("IR peak", levels.map(|v| v.0.peak_r), "Input right peak, dBFS."),
            DetailCell::InputRmsLeft => reading("IL RMS", levels.map(|v| v.0.rms_l), "Input left RMS, dBFS."),
            DetailCell::InputRmsRight => reading("IR RMS", levels.map(|v| v.0.rms_r), "Input right RMS, dBFS."),
            DetailCell::OutputPeakLeft => reading("OL peak", levels.map(|v| v.1.peak_l), "Output left peak, dBFS."),
            DetailCell::OutputPeakRight => reading("OR peak", levels.map(|v| v.1.peak_r), "Output right peak, dBFS."),
            DetailCell::OutputRmsLeft => reading("OL RMS", levels.map(|v| v.1.rms_l), "Output left RMS, dBFS."),
            DetailCell::OutputRmsRight => reading("OR RMS", levels.map(|v| v.1.rms_r), "Output right RMS, dBFS."),
        };
        container(content).width(bounds.width).height(bounds.height).into()
    })).spacing(6);
    let selector = page_selector(track, effect, layout.page, accent);
    let content = container(cells).width(layout.content.width).height(layout.content.height);
    let pages: Element<'a, Message> = if layout.content.y > layout.selector.y + 1.0 {
        column![selector, content].spacing(6).into()
    } else {
        row![selector, content].spacing(8).align_y(iced::Alignment::Center).into()
    };
    container(pages).width(layout.bounds.width).height(layout.bounds.height)
        .align_y(iced::Alignment::Center).into()
}
