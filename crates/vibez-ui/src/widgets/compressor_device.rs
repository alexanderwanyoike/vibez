//! Measures Compressor composition around persistent core controls and a dominant transfer plot.

use crate::widgets::{
    compressor_controls,
    compressor_display::{ReductionGauge, ReductionHistory, TransferCurve},
    device_chrome::section_label,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{
    domains::{
        compressor::{CompressorMsg, CompressorPanel, CompressorVisual},
        compressor_layout::{
            self, plot_widths, ASIDE_W, GAUGE_W, INLINE_CORE_W, SIDECHAIN_W, TOOLBAR_H,
        },
        device_controls::{DeviceControlMsg, DeviceControlState},
        devices::DevicesState,
    },
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
};
use iced::{
    widget::{button, canvas, column, container, horizontal_space, row, text},
    Color, Element, Length, Size,
};
use vibez_core::id::TrackId;
use vibez_engine::metering::MeterValues;

pub use crate::domains::compressor_layout::card_width;

pub fn preset_bar_width(size: Size) -> f32 {
    compressor_layout::preset_bar_width(size.width, size.height)
}

fn details_button<'a>(
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

fn db_reading(value: f32) -> String {
    let db = vibez_core::level::amplitude_to_db(value);
    if db.is_finite() {
        format!("{db:.1}")
    } else {
        "-inf".into()
    }
}

fn signal_meter<'a>(
    label: &'static str,
    levels: MeterValues,
    numbers: bool,
) -> Element<'a, Message> {
    let mut content = column![
        section_label(label),
        canvas(LevelMeter { levels })
            .width(METER_W)
            .height(Length::Fill)
    ]
    .spacing(2)
    .width(50.0)
    .height(Length::Fill);
    if numbers {
        for (channel, peak) in [("L", levels.peak_l), ("R", levels.peak_r)] {
            content = content.push(
                text(format!("{channel} {}", db_reading(peak)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
    }
    content.into()
}

fn signal_readouts<'a>(input: MeterValues, output: MeterValues) -> Element<'a, Message> {
    let mut content = column![].spacing(2);
    for (label, levels) in [("In", input), ("Out", output)] {
        content = content.push(
            text(format!(
                "{label} L {} R {}",
                db_reading(levels.peak_l),
                db_reading(levels.peak_r)
            ))
            .size(8)
            .font(crate::typography::PERFORM_TECH)
            .color(th::text_dim())
            .wrapping(iced::widget::text::Wrapping::None),
        );
    }
    content.into()
}

fn visual_switch<'a>(
    track: TrackId,
    effect: &UiEffect,
    panel: &CompressorPanel,
    accent: Color,
) -> Element<'a, Message> {
    let mut tabs = row![].spacing(3);
    for (label, visual) in [
        ("Transfer", CompressorVisual::Transfer),
        ("History", CompressorVisual::History),
    ] {
        let selected = panel.visual == visual;
        tabs = tabs.push(
            button(text(label).size(10))
                .width(Length::Fill)
                .height(24.0)
                .padding([2, 5])
                .on_press(Message::CompressorControl(CompressorMsg::Visual(
                    track, effect.id, visual,
                )))
                .style(move |_, _| button::Style {
                    text_color: if selected { accent } else { th::text_dim() },
                    background: Some(
                        if selected {
                            th::bg_hover()
                        } else {
                            th::bg_dark()
                        }
                        .into(),
                    ),
                    border: iced::Border {
                        color: if selected { accent } else { th::border() },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }),
        );
    }
    tabs.width(Length::Fill).into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    panel: &'a CompressorPanel,
    preset: Element<'a, Message>,
    input: MeterValues,
    output: MeterValues,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    devices: &DevicesState,
    size: Size,
) -> Element<'a, Message> {
    let layout = compressor_layout::arrange(size.width, size.height, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let current = if effect.bypass {
        [0.0; 2]
    } else {
        panel.meter.current_reduction_db
    };
    let peak = if effect.bypass {
        [0.0; 2]
    } else {
        panel.meter.peak_reduction_db
    };
    let gr = current[0].max(current[1]);
    let mut toolbar = row![preset, horizontal_space()]
        .spacing(5)
        .align_y(iced::Alignment::Center);
    if size.height < 160.0 {
        toolbar = toolbar.push(
            text(format!("GR {gr:.1} dB"))
                .size(9)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
        );
        if state.advanced && (!layout.primary_visible_with_details || layout.inline_details()) {
            toolbar = toolbar.push(details_button(track, effect, true, layout.compact, accent));
        }
    }
    let mut body = column![container(toolbar).padding([0, 4]).height(TOOLBAR_H)];
    if layout.tabs > 0.0 {
        body = body.push(
            container(visual_switch(track, effect, panel, accent))
                .height(layout.tabs)
                .padding([0, 4]),
        );
    }
    if layout.routing > 0.0 {
        body = body.push(
            container(crate::widgets::native_sidechain::view(
                track, effect, sources, buses, devices, panel, accent, false,
            ))
            .height(layout.routing)
            .padding([2, 4]),
        );
    }
    if layout.plot > 0.0 {
        let (transfer_width, history_width) = plot_widths(size.width, layout);
        let transfer = || -> Element<'a, Message> {
            let canvas_height = layout.plot - 4.0 - (8.0 * 1.3 + 3.0);
            let caption = if canvas_height < 80.0 {
                "Manual internal · Output dB ↑ / Input dB →"
            } else {
                "Static transfer · manual internal detector"
            };
            column![section_label(caption)]
                .spacing(3)
                .push(
                    canvas(TransferCurve {
                        track,
                        effect,
                        accent,
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                )
                .width(transfer_width)
                .height(Length::Fill)
                .into()
        };
        let history = || -> Element<'a, Message> {
            column![
                section_label("Window peak wet GR · dB"),
                canvas(ReductionHistory {
                    values: panel.history().collect(),
                    accent,
                })
                .width(Length::Fill)
                .height(Length::Fill)
            ]
            .spacing(3)
            .width(if layout.both_plots {
                history_width
            } else {
                transfer_width
            })
            .height(Length::Fill)
            .into()
        };
        let plots: Element<'a, Message> = if layout.both_plots {
            row![transfer(), history()]
                .spacing(8)
                .height(Length::Fill)
                .into()
        } else if panel.visual == CompressorVisual::Transfer {
            transfer()
        } else {
            history()
        };
        let mut visual = row![].spacing(8).height(Length::Fill);
        if layout.sidechain_column {
            let source = crate::widgets::native_sidechain::view(
                track, effect, sources, buses, devices, panel, accent, true,
            );
            let meters: Element<'a, Message> = if layout.plot >= 180.0
                || effect.sidechains.is_empty() && effect.inactive_sidechains.is_empty()
            {
                row![
                    signal_meter("Input dBFS", input, layout.plot >= 180.0),
                    signal_meter("Output dBFS", output, layout.plot >= 180.0)
                ]
                .spacing(8)
                .height(Length::Fill)
                .into()
            } else {
                signal_readouts(input, output)
            };
            visual = visual.push(
                container(column![source, meters].spacing(6).height(Length::Fill))
                    .width(SIDECHAIN_W),
            );
        }
        let mut gauge = column![].spacing(3).width(GAUGE_W).height(Length::Fill);
        if layout.plot >= 100.0 {
            gauge = gauge.push(section_label("Reduction"));
        }
        gauge = gauge
            .push(
                text(format!("GR {gr:.1} dB"))
                    .size(if layout.plot >= 100.0 { 11 } else { 9 })
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .push(
                canvas(ReductionGauge {
                    current,
                    peak,
                    accent,
                })
                .width(50.0)
                .height(Length::Fill),
            );
        if layout.plot >= 100.0 {
            gauge = gauge.push(
                text(format!("L {:.1}  R {:.1}", current[0], current[1]))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
        body = body.push(
            container(visual.push(plots).push(gauge))
                .height(layout.plot)
                .padding([2, 4]),
        );
    }
    let advanced = || {
        compressor_controls::advanced(
            track, effect, state, panel, accent, sources, buses, devices, input, output,
        )
    };
    let primary = layout.primary_controls();
    let controls: Element<'a, Message> = if layout.expanded {
        row![
            compressor_controls::primary(track, effect, state, accent, layout.knobs, primary),
            iced::widget::vertical_rule(1),
            container(compressor_controls::aside(
                track,
                effect,
                state,
                panel,
                accent,
                input,
                output,
                details_button(track, effect, state.advanced, false, accent),
            ))
            .width(ASIDE_W)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.inline_details() {
        row![
            container(compressor_controls::primary(
                track, effect, state, accent, None, primary,
            ))
            .width(INLINE_CORE_W),
            iced::widget::vertical_rule(1),
            advanced(),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else if state.advanced && !layout.primary_visible_with_details {
        advanced()
    } else {
        row![
            compressor_controls::primary(track, effect, state, accent, layout.knobs, primary),
            details_button(track, effect, state.advanced, layout.compact, accent)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    };
    body = body.push(iced::widget::horizontal_rule(1)).push(
        container(controls)
            .padding([2, 4])
            .height(layout.controls - 1.0)
            .align_y(iced::alignment::Vertical::Center),
    );
    if layout.details > 0.0 {
        let details = advanced();
        body = body.push(
            container(details)
                .height(layout.details)
                .padding([3, 4])
                .align_y(iced::alignment::Vertical::Center),
        );
    }
    body.width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_axes_keep_a_real_plot_after_compact_axis_qualification() {
        for width in [430.0, 620.0, 900.0] {
            for height in [
                64.0, 114.0, 116.0, 120.0, 130.0, 138.0, 140.0, 146.0, 160.0, 182.0, 203.0, 240.0,
                304.0,
            ] {
                let layout = compressor_layout::arrange(width, height, false);
                if layout.plot == 0.0 {
                    continue;
                }
                let caption = 8.0 * 1.3 + 3.0;
                let available = layout.plot - 4.0 - caption;
                let canvas_height = available;
                let actual = crate::widgets::compressor_display::transfer_plot(
                    iced::Rectangle::new(iced::Point::ORIGIN, Size::new(240.0, canvas_height)),
                )
                .height;
                assert!(
                    actual >= 32.0,
                    "named axes leave only {actual}px at {width}x{height}"
                );
            }
        }
    }
}
