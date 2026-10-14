//! Composes the responsive native Bitcrush body from measured scope, meter and control regions.

use crate::{
    domains::{
        bitcrush_layout::{self, DetailPage},
        device_controls::{DeviceControlState, ParameterPresentation},
    },
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        bitcrush_controls::{self, decibels},
        bitcrush_display::SampleHoldScope,
        device_chrome::section_label,
        device_controls::disclosure_with_label,
        level_meter::LevelMeter,
    },
};
use iced::{widget::{canvas, column, container, row, text, tooltip, Space}, Color, Element, Length, Size};
use vibez_core::{effect::SampleHoldMeter, id::TrackId};
use vibez_engine::metering::MeterValues;

pub use crate::domains::bitcrush_layout::card_width;

pub fn preset_bar_width(size: Size) -> f32 {
    bitcrush_layout::preset_bar_width(size.width)
}

fn signal_meter<'a>(label: &'static str, levels: Option<MeterValues>) -> Element<'a, Message> {
    tooltip(
        column![
            section_label(label),
            canvas(LevelMeter { levels: levels.unwrap_or(MeterValues::SILENT) })
                .width(crate::widgets::level_meter::WIDTH).height(Length::Fill),
            text(format!("L {}", decibels(levels.map(|value| value.peak_l))))
                .size(8).font(crate::typography::PERFORM_TECH).wrapping(iced::widget::text::Wrapping::None),
            text(format!("R {}", decibels(levels.map(|value| value.peak_r))))
                .size(8).font(crate::typography::PERFORM_TECH).wrapping(iced::widget::text::Wrapping::None)
        ].spacing(2).width(52.0).height(Length::Fill),
        text("Measured stereo levels in dBFS. Bars show RMS and peak marks; L/R values show peaks. N/A means no current meter packet, while -inf is genuine measured silence.").size(9),
        tooltip::Position::Top,
    ).into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    page: DetailPage,
    presets: Element<'a, Message>,
    presentations: &[ParameterPresentation],
    meter: Option<SampleHoldMeter>,
    levels: Option<(MeterValues, MeterValues)>,
    size: Size,
) -> Element<'a, Message> {
    let layout = bitcrush_layout::arrange(size.width, size.height, state.advanced, page);
    let accent = if effect.bypass { th::text_muted() } else { accent };
    let available = !effect.bypass && presentations
        .get(vibez_dsp::bitcrush::param::SR_REDUCE)
        .is_some_and(ParameterPresentation::is_editable);
    let meter = meter.filter(|_| available);
    let levels = levels.filter(|_| available);
    let primary = || bitcrush_controls::primary(track, effect, state, accent, &layout.core, presentations);
    let details = || bitcrush_controls::details(
        track, effect, state, accent, layout.details.as_ref().unwrap(), presentations, meter, levels,
    );
    let toolbar = container(row![
        container(presets).width(layout.presets.width).height(layout.presets.height),
        container(disclosure_with_label(track, effect, state.advanced, "Details"))
            .width(layout.disclosure.width).height(layout.disclosure.height)
            .align_y(iced::Alignment::Center)
    ].spacing(8)).padding([0, 4]).height(26.0);
    let body_height = size.height - 28.0;
    let body: Element<'a, Message> = if let Some(scope) = layout.scope {
        let waveform = tooltip(
            canvas(SampleHoldScope { meter, accent }).width(scope.width).height(scope.height),
            text("Actual sanitized LEFT input before Drive and final mixed output. Lines connect adjacent measured samples, including dry Mix and Post LP; no artificial staircase. Time uses this packet's count and sample rate. Ticks1/2 mark historical primary/secondary acquisitions, tint marks scheduled transition frames. f is the absolute first source frame. Each packet is a separate measured window.").size(9),
            tooltip::Position::Top,
        );
        let plot_bank: Element<'a, Message> = if let (Some(input), Some(output)) = (layout.input_meter, layout.output_meter) {
            row![
                container(signal_meter("Input", levels.map(|v| v.0))).width(input.width).height(input.height),
                Space::with_width(6.0), waveform, Space::with_width(6.0),
                container(signal_meter("Output", levels.map(|v| v.1))).width(output.width).height(output.height)
            ].height(scope.height).into()
        } else {
            waveform.into()
        };
        if let Some(divider) = layout.divider {
            let core_y = layout.core[0].bounds.y;
            let mut controls = column![Space::with_height(core_y - 28.0), primary()];
            if layout.details.is_some() {
                controls = controls.push(Space::with_height(6.0)).push(details());
            }
            row![plot_bank, Space::with_width(8.0),
                iced::widget::vertical_rule(divider.width), Space::with_width(8.0),
                container(controls).width(296.0).height(body_height)]
                .height(body_height).into()
        } else {
            let mut content = column![plot_bank, Space::with_height(6.0), primary()];
            if layout.details.is_some() {
                content = content.push(Space::with_height(6.0)).push(details());
            }
            content.height(body_height).into()
        }
    } else if let Some(divider) = layout.divider {
        row![
            container(primary()).width(290.0).height(body_height).align_y(iced::Alignment::Center),
            Space::with_width(8.0), iced::widget::vertical_rule(divider.width), Space::with_width(8.0),
            details()
        ].height(body_height).into()
    } else if layout.core.is_empty() {
        details()
    } else {
        let mut content = column![Space::with_height(layout.core[0].bounds.y - 28.0), primary()];
        if layout.details.is_some() {
            content = content.push(Space::with_height(6.0)).push(details());
        }
        content.height(body_height).into()
    };
    column![toolbar, Space::with_height(2.0), container(body).padding([0, 4]).height(body_height)]
        .width(Length::Fill).height(Length::Fill).into()
}
