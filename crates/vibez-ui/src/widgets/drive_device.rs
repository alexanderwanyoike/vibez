//! Composes Drive's measured plot, compact control bank and native detail regions.

use crate::{
    domains::{
        device_controls::DeviceControlState,
        drive::DrivePanel,
        drive_layout::{self, ASIDE_WIDTH, COMPACT_BANK_WIDTH, INLINE_CORE_WIDTH, METER_WIDTH},
    },
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        device_chrome::section_label,
        drive_controls::{self, db},
        drive_display::{PairedWaveform, TransferCurve},
        level_meter::LevelMeter,
    },
};
use iced::{
    widget::{canvas, column, container, horizontal_space, row, text, tooltip},
    Color, Element, Length, Size,
};
use vibez_core::{effect::WaveShaperMeter, id::TrackId};
use vibez_engine::metering::MeterValues;

pub use crate::domains::drive_layout::card_width;

pub fn preset_bar_width(size: Size) -> f32 {
    drive_layout::preset_bar_width(size.width, size.height)
}

fn signal_meter<'a>(label: &'static str, levels: MeterValues) -> Element<'a, Message> {
    tooltip(
        column![
            section_label(label),
            canvas(LevelMeter { levels })
                .width(crate::widgets::level_meter::WIDTH)
                .height(Length::Fill),
            text(format!("L {}", db(levels.peak_l)))
                .size(8)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
            text(format!("R {}", db(levels.peak_r)))
                .size(8)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(2)
        .width(METER_WIDTH)
        .height(Length::Fill),
        text("Stereo levels in dBFS. L/R numeric readings show channel peaks.").size(9),
        tooltip::Position::Top,
    )
    .into()
}

fn transfer<'a>(meter: Option<&'a WaveShaperMeter>, accent: Color) -> Element<'a, Message> {
    column![section_label("Static transfer"),
        tooltip(canvas(TransferCurve { meter, accent }).width(Length::Fill).height(Length::Fill),
            text("Signed linear input and output amplitudes from the applied DSP controls. This memoryless curve excludes Tone, the pre/post filters and resampling memory.").size(9),
            tooltip::Position::Top)]
        .spacing(3).height(Length::Fill).into()
}

fn waveform<'a>(meter: Option<&'a WaveShaperMeter>, accent: Color) -> Element<'a, Message> {
    let label = meter.map_or("Aligned left In / Out".into(), |meter| {
        format!("Left In / Out · {:.1} kHz", meter.sample_rate / 1000.0)
    });
    column![section_label(&label),
        tooltip(canvas(PairedWaveform { meter, accent }).width(Length::Fill).height(Length::Fill),
            text("Actual left-channel sample pairs: gray input and accented output, aligned by the reported DSP latency. The horizontal labels are output sample positions. Silence is a flat trace; missing or stale telemetry is unavailable.").size(9),
            tooltip::Position::Top)]
        .spacing(3).height(Length::Fill).into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    panel: &'a DrivePanel,
    presets: Element<'a, Message>,
    input: MeterValues,
    output: MeterValues,
    size: Size,
) -> Element<'a, Message> {
    let layout = drive_layout::arrange(size.width, size.height, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let meter = if effect.bypass {
        None
    } else {
        panel.meter.as_ref()
    };
    let compact = size.width < drive_layout::MEDIUM_WIDTH;
    let disclosure = || drive_controls::disclosure(track, effect, state.advanced, compact);
    let primary = || {
        drive_controls::primary(
            track,
            effect,
            state,
            accent,
            layout.knob_size,
            layout.primary_rows,
        )
    };
    let advanced = || drive_controls::advanced(track, effect, state, panel, accent, input, output);
    let mut toolbar = row![presets, horizontal_space()]
        .spacing(5)
        .align_y(iced::Alignment::Center);
    if state.advanced && (!layout.primary_visible || layout.inline_details) {
        toolbar = toolbar.push(disclosure());
    }
    let mut content = column![container(toolbar).padding([0, 4]).height(layout.toolbar)];
    if layout.body > 0.0 {
        let plots: Element<'a, Message> = if layout.compact_split {
            row![
                container(transfer(meter, accent)).width(layout.transfer_width),
                container(primary())
                    .width(COMPACT_BANK_WIDTH)
                    .height(layout.primary_height)
            ]
            .spacing(12)
            .height(Length::Fill)
            .into()
        } else {
            let mut plots = row![].spacing(8).height(Length::Fill);
            if layout.input_meter {
                plots = plots.push(signal_meter("Input", input));
            }
            plots = plots.push(container(transfer(meter, accent)).width(layout.transfer_width));
            if layout.both_plots {
                plots = plots.push(container(waveform(meter, accent)).width(layout.waveform_width));
            }
            if layout.output_meter {
                plots = plots.push(signal_meter("Output", output));
            }
            if layout.advanced_aside {
                plots = plots.push(
                    container(drive_controls::aside(track, effect, state, accent))
                        .padding([2, 4])
                        .width(ASIDE_WIDTH),
                );
            }
            plots.into()
        };
        content = content.push(container(plots).padding([2, 4]).height(layout.body));
    }
    let footer: Element<'a, Message> = if layout.compact_split {
        row![
            container(drive_controls::character(track, effect, accent)).width(112.0),
            horizontal_space(),
            disclosure()
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.inline_details {
        row![
            container(primary()).width(INLINE_CORE_WIDTH),
            iced::widget::vertical_rule(1),
            container(advanced()).width(layout.details_width)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.primary_visible {
        let mut controls = row![container(primary()).width(layout.primary_width)]
            .spacing(if layout.character_visible { 12 } else { 8 })
            .align_y(iced::Alignment::Center);
        if layout.character_visible {
            controls = controls
                .push(iced::widget::vertical_rule(1))
                .push(container(drive_controls::character(track, effect, accent)).width(112.0));
        }
        controls = controls.push(horizontal_space());
        if !layout.advanced_aside {
            controls = controls.push(disclosure());
        }
        controls.into()
    } else {
        advanced()
    };
    content = content.push(iced::widget::horizontal_rule(1)).push(
        container(footer)
            .height(layout.footer - 1.0)
            .padding([2, 4])
            .align_y(iced::Alignment::Center),
    );
    if layout.details > 0.0 {
        content = content.push(
            container(advanced())
                .height(layout.details)
                .padding([3, 4])
                .align_y(iced::Alignment::Center),
        );
    }
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
