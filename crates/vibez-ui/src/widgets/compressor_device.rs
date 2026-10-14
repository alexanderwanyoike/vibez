//! Measures Compressor composition around persistent core controls and a dominant transfer plot.

use crate::widgets::{
    compressor_controls::{self, PrimaryControls},
    compressor_display::{ReductionGauge, ReductionHistory, TransferCurve},
    device_chrome::section_label,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{
    domains::{
        compressor::{CompressorMsg, CompressorPanel, CompressorVisual},
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

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 740.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;
const SIDECHAIN_W: f32 = 120.0;
const GAUGE_W: f32 = 76.0;

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub plot: f32,
    pub controls: f32,
    pub knobs: Option<f32>,
    pub primary_beside: bool,
    pub sidechain_column: bool,
    pub both_plots: bool,
    pub compact: bool,
    pub expanded: bool,
    pub tabs: f32,
    pub details: f32,
    pub routing: f32,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let compact = size.width < MEDIUM_W;
    let expanded = size.width >= 872.0;
    let (controls, knobs) = if size.height >= 280.0 {
        (84.0, Some(34.0))
    } else if size.height >= 160.0 {
        (57.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let details = if advanced && size.height >= 112.0 {
        if size.height >= 280.0 {
            44.0
        } else {
            36.0
        }
    } else {
        0.0
    };
    let available = (size.height - TOOLBAR_H - controls - details).max(0.0);
    let tabs = if compact && available >= 108.0 {
        24.0
    } else {
        0.0
    };
    let sidechain_column = !compact && available >= 150.0;
    let routing = if !compact && !sidechain_column && available >= 120.0 {
        36.0
    } else {
        0.0
    };
    let available = available - tabs - routing;
    let plot = if available >= 84.0 { available } else { 0.0 };
    Arrangement {
        plot,
        controls,
        knobs,
        primary_beside: advanced && details > 0.0,
        sidechain_column,
        both_plots: !compact && plot > 0.0,
        compact,
        expanded,
        tabs,
        details,
        routing,
    }
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - if size.height < 160.0 { 190.0 } else { 8.0 }
}

fn plot_widths(width: f32, layout: Arrangement) -> (f32, f32) {
    let source = if layout.sidechain_column {
        SIDECHAIN_W + 8.0
    } else {
        0.0
    };
    let available = (width - 8.0 - source - GAUGE_W - 8.0).max(32.0);
    if layout.both_plots {
        let available = available - 8.0;
        (available * 2.0 / 3.0, available / 3.0)
    } else {
        (available, 0.0)
    }
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

fn signal_meter<'a>(
    label: &'static str,
    levels: MeterValues,
    numbers: bool,
) -> Element<'a, Message> {
    let reading = |value| {
        let db = vibez_core::level::amplitude_to_db(value);
        if db.is_finite() {
            format!("{db:.1}")
        } else {
            "-inf".into()
        }
    };
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
                text(format!("{channel} {}", reading(peak)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
    }
    content.into()
}

fn signal_readouts<'a>(input: MeterValues, output: MeterValues) -> Element<'a, Message> {
    let reading = |value| {
        let db = vibez_core::level::amplitude_to_db(value);
        if db.is_finite() { format!("{db:.1}") } else { "-inf".into() }
    };
    let mut content = column![].spacing(2);
    for (label, levels) in [("In", input), ("Out", output)] {
        content = content.push(text(format!("{label} L {} R {}", reading(levels.peak_l), reading(levels.peak_r)))
            .size(8).font(crate::typography::PERFORM_TECH).color(th::text_dim())
            .wrapping(iced::widget::text::Wrapping::None));
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
    let layout = arrange(size, state.advanced);
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
        if state.advanced && !layout.primary_beside {
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
            let mut content =
                column![section_label("Static transfer · manual internal detector")].spacing(3);
            let canvas_height = layout.plot - 4.0 - (8.0 * 1.3 + 3.0);
            if canvas_height < 80.0 {
                content = content.push(section_label("Output dB ↑ · Input dB →"));
            }
            content
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
                ].spacing(8).height(Length::Fill).into()
            } else {
                signal_readouts(input, output)
            };
            visual = visual.push(
                container(column![source, meters].spacing(6).height(Length::Fill))
                    .width(SIDECHAIN_W),
            );
        }
        let gauge = column![
            section_label("Reduction"),
            text(format!("GR {gr:.1} dB"))
                .size(11)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
            canvas(ReductionGauge {
                current,
                peak,
                accent
            })
            .width(50.0)
            .height(Length::Fill),
            text(format!("L {:.1}  R {:.1}", current[0], current[1]))
                .size(8)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(3)
        .width(GAUGE_W)
        .height(Length::Fill);
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
    let primary = if layout.compact {
        PrimaryControls::Core
    } else if layout.expanded {
        PrimaryControls::Expanded
    } else {
        PrimaryControls::Full
    };
    let controls: Element<'a, Message> = if state.advanced && !layout.primary_beside {
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
        let details = if layout.expanded {
            compressor_controls::strip(track, effect, state, accent)
        } else {
            advanced()
        };
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
    fn measured_compressor_tiers_fit_actual_control_and_caption_geometry() {
        for width in [430.0, 620.0, 900.0] {
            for height in [
                64.0, 72.0, 90.0, 108.0, 113.0, 114.0, 130.0, 160.0, 240.0, 304.0, 400.0,
            ] {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    assert!(
                        TOOLBAR_H
                            + layout.plot
                            + layout.controls
                            + layout.tabs
                            + layout.details
                            + layout.routing
                            <= height + 0.001,
                        "{width}x{height} Advanced={advanced}"
                    );
                    let field = 8.0 * 1.3
                        + 3.0
                        + 10.0 * 1.3
                        + 2.0
                        + layout.knobs.map_or(0.0, |size| size + 3.0);
                    assert!(
                        layout.controls >= field,
                        "controls clip at {width}x{height}"
                    );
                    assert!(layout.controls >= 36.0);
                    let caption = 8.0 * 1.3 + 3.0;
                    assert!(
                        layout.plot == 0.0 || layout.plot >= 4.0 + caption + 32.0,
                        "no miniature plots"
                    );
                    assert_eq!(layout.primary_beside, advanced && height >= 112.0);
                    assert!(!layout.sidechain_column || width >= MEDIUM_W && layout.plot >= 150.0);
                    assert!(!layout.both_plots || width >= MEDIUM_W);
                    if layout.plot > 0.0 {
                        assert!(
                            layout.plot - 4.0 - (8.0 * 1.3 + 9.0 * 1.3 + 6.0) >= 18.0,
                            "GR gauge loses its usable height"
                        );
                    }
                    if layout.plot >= 100.0 {
                        assert!(
                            layout.plot - 4.0 - (3.0 * 8.0 * 1.3 + 6.0) >= 32.0,
                            "signal meter loses its usable height"
                        );
                    }
                    if layout.sidechain_column {
                        assert!(
                            layout.plot - 4.0
                                >= 2.0 * (8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0)
                                    + 9.0 * 1.3
                                    + 4.0
                                    + 2.0 * 8.0 * 1.3
                                    + 20.0
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn compressor_width_uses_the_chain_remainder_with_readable_bounds() {
        assert_eq!(card_width(80.0), COMPACT_W);
        assert_eq!(card_width(710.0), 710.0);
        assert_eq!(card_width(1800.0), MAX_W);
    }

    #[test]
    fn sidechain_column_reserves_source_tap_input_disconnect_warning_and_raw_peak() {
        let field = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
        let disconnect = 9.0 * 1.3 + 4.0;
        let warnings_and_raw_peak = 2.0 * 8.0 * 1.3;
        let four_gaps = 4.0 * 5.0;
        let child_content = 2.0 * field + disconnect + warnings_and_raw_peak + four_gaps;
        for height in [251.0, 252.0, 253.0, 254.0, 260.0, 262.0] {
            let layout = arrange(Size::new(900.0, height), true);
            assert!(
                !layout.sidechain_column || layout.plot - 4.0 >= child_content,
                "sidechain fields clip at body height {height}: {} available for {child_content}",
                layout.plot - 4.0
            );
        }
    }

    #[test]
    fn transfer_axes_keep_a_real_plot_after_compact_axis_qualification() {
        for width in [430.0, 620.0, 900.0] {
            for height in [
                64.0, 114.0, 116.0, 120.0, 130.0, 138.0, 140.0, 160.0, 182.0, 240.0, 304.0,
            ] {
                let layout = arrange(Size::new(width, height), false);
                if layout.plot == 0.0 {
                    continue;
                }
                let caption = 8.0 * 1.3 + 3.0;
                let available = layout.plot - 4.0 - caption;
                let canvas_height = if available < 80.0 {
                    available - (8.0 * 1.3 + 3.0)
                } else {
                    available
                };
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

    #[test]
    fn advanced_preserves_primary_controls_at_the_actual_maximum_body_in_every_tier() {
        for width in [430.0, 472.0, 740.0, 804.0, 872.0, 892.0] {
            let layout = arrange(Size::new(width, 304.0), true);
            assert!(
                layout.primary_beside,
                "Advanced replaces the core controls at width {width}"
            );
            let child_height = 8.0 * 1.3 + 3.0 + 34.0 + 3.0 + 10.0 * 1.3 + 2.0;
            assert!(
                layout.controls >= child_height + 4.0,
                "core knobs and values clip at width {width}"
            );
            assert!(
                layout.plot >= 100.0,
                "the defining visual disappears at {width}"
            );
        }
    }

    #[test]
    fn medium_keeps_sidechain_source_visible_before_opening_details() {
        for width in [740.0, 804.0, 820.0, 871.0] {
            let layout = arrange(Size::new(width, 304.0), false);
            assert!(
                layout.sidechain_column,
                "sidechain hidden at medium width {width}"
            );
            assert!(
                layout.plot >= 150.0,
                "sidechain has no real vertical budget"
            );
        }
    }

    #[test]
    fn composition_boundaries_keep_a_landscape_transfer_and_a_supporting_history() {
        for width in [430.0, 472.0, 739.0, 740.0, 804.0, 871.0, 872.0, 892.0] {
            for advanced in [false, true] {
                let layout = arrange(Size::new(width, 304.0), advanced);
                let (transfer, history) = plot_widths(width, layout);
                if width < MEDIUM_W {
                    assert_eq!(layout.tabs, 24.0);
                    assert_eq!(history, 0.0);
                    assert!(transfer >= 320.0);
                } else {
                    assert!(layout.sidechain_column);
                    assert!(transfer >= 320.0, "transfer too narrow at {width}");
                    assert!(history >= 150.0, "history too narrow at {width}");
                    assert!((transfer - history * 2.0).abs() < 0.001);
                }
                assert_eq!(layout.expanded, width >= 872.0);
                let child = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
                if advanced {
                    assert!(layout.details - 6.0 >= child);
                }
            }
        }
    }

    #[test]
    fn primary_children_fit_after_separator_and_container_padding_at_intermediate_heights() {
        for height in [160.0, 180.0, 219.0, 240.0, 279.0, 280.0, 304.0] {
            for width in [430.0, 740.0, 872.0] {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let child = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0
                        + layout.knobs.map_or(0.0, |knob| knob + 3.0);
                    let available = layout.controls - 1.0 - 2.0 * 2.0;
                    assert!(available >= child,
                        "primary child needs {child}px but only {available}px remains at {width}x{height}");
                }
            }
        }
    }

}
