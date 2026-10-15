//! Composes Delay's native response, live output and musical control strip from measured regions.

use iced::{widget::{canvas, column, container, row, text, tooltip, Space}, Color, Element, Length, Size};
use vibez_core::{id::{EffectId, TrackId}, level::amplitude_to_db};
use vibez_engine::metering::MeterValues;
use crate::{domains::{delay_layout::{self, Page}, delay_observation::DelayObservation,
    delay_response::DelayResponse, device_controls::DeviceControlState}, message::Message,
    state::UiEffect, theme as th, widgets::{delay_controls, delay_display::DelayDisplay,
        device_chrome::section_label, device_controls::disclosure, level_meter::LevelMeter}};

pub use delay_layout::card_width;

pub fn preset_bar_width(size: Size) -> f32 { size.width - 104.0 }

pub struct DelayView<'a> {
    pub track: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub controls: &'a DeviceControlState,
    pub observation: &'a DelayObservation,
    pub response: Option<&'a DelayResponse>,
    pub response_status: &'a str,
    pub output: Option<MeterValues>,
    pub presets: Element<'a, Message>,
    pub size: Size,
    pub on_page: fn(TrackId, EffectId, Page) -> Message,
}

fn peak(value: f32) -> String {
    let db = amplitude_to_db(value);
    if db.is_finite() { format!("{db:.0}") } else { "-inf".into() }
}

fn output_meter<'a>(levels: Option<MeterValues>, duck_db: Option<f32>, height: f32) -> Element<'a, Message> {
    let reading = levels.map_or_else(|| "N/A".into(), |v| format!("{}/{}", peak(v.peak_l), peak(v.peak_r)));
    let meter: Element<'a, Message> = if let Some(levels) = levels {
        canvas(LevelMeter { levels }).width(crate::widgets::level_meter::WIDTH)
            .height(Length::Fill).into()
    } else {
        container(text("N/A").size(8).color(th::text_muted())).height(Length::Fill).into()
    };
    let detail = levels.map_or_else(|| "Live output unavailable".into(), |v|
        format!("Live post-Mix stereo dBFS. Peak L {:.1}, R {:.1}. Bars show RMS; marks show peak.",
            amplitude_to_db(v.peak_l), amplitude_to_db(v.peak_r)));
    let detail = format!("{detail} Wet-only Duck attenuation: {} dB.",
        duck_db.map_or_else(|| "N/A".into(), |db| format!("{db:.1}")));
    tooltip(column![section_label("Output L/R"), meter,
        text(reading).size(8).font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None)]
        .spacing(3).width(52.0).height(height), text(detail).size(9).width(250.0),
        tooltip::Position::Top).into()
}

pub fn view<'a>(args: DelayView<'a>) -> Element<'a, Message> {
    let DelayView { track, effect, accent, controls, observation, response, response_status,
        output, presets, size, on_page } = args;
    let layout = delay_layout::arrange(size.width, size.height, controls.advanced, observation.page);
    let accent = if effect.bypass { th::text_muted() } else { accent };
    let available = !effect.bypass && !effect.reconfiguration_failed;
    let snapshot = observation.snapshot.as_ref().filter(|_| available);
    let response = response.filter(|_| snapshot.is_some_and(|state| !state.reference_moving));
    let toolbar = row![container(presets).width(layout.presets.width).height(layout.toolbar.height),
        container(disclosure(track, effect, controls.advanced)).width(layout.disclosure.width)
            .height(layout.toolbar.height).align_y(iced::Alignment::Center)].spacing(8);
    let mut content = column![];
    if let Some(graph) = layout.graph {
        let display = canvas(DelayDisplay { response, snapshot, layout: graph, accent,
            status: if snapshot.is_none() { "Unavailable" } else { response_status } })
            .width(graph.bounds.width).height(graph.bounds.height);
        let mut plot = row![display];
        if let Some(meter) = layout.output_meter {
            plot = plot.push(Space::with_width(6.0)).push(output_meter(
                output.filter(|_| available), snapshot.map(|state| state.duck_attenuation_db), meter.height));
        }
        content = content.push(plot).push(Space::with_height(6.0));
    }
    if let Some(first) = layout.main.first() {
        content = content.push(container(delay_controls::bank(track, effect, controls, accent, &layout.main, snapshot))
            .height(first.bounds.height));
        if layout.selector.is_some() { content = content.push(Space::with_height(6.0)); }
    }
    if let Some(selector) = layout.selector {
        let id = effect.id;
        content = content.push(row![
            container(delay_controls::page_selector(observation.page,
                move |page| on_page(track, id, page), accent))
                .width(selector.width).height(selector.height),
            Space::with_width(8.0),
            delay_controls::bank(track, effect, controls, accent, &layout.details, snapshot)
        ].height(selector.height));
    }
    column![container(toolbar).padding([0, 4]).height(layout.toolbar.height), Space::with_height(2.0),
        container(content).padding([0, 4]).height(size.height - layout.toolbar.height - 2.0)]
        .width(Length::Fill).height(Length::Fill).into()
}
