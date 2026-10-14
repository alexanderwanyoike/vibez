//! Arranges native Compressor controls and measured dynamics displays by available size.

use crate::widgets::{
    compressor_display::{ReductionGauge, ReductionHistory, TransferCurve},
    device_chrome::section_label,
    device_controls::disclosure,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{
    domains::{
        compressor::{CompressorMsg, CompressorPanel, CompressorVisual},
        device_controls::DeviceControlState,
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
pub const MEDIUM_W: f32 = 620.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub plot: f32,
    pub controls: f32,
    pub knobs: Option<f32>,
    pub primary_beside: bool,
    pub sidechain_column: bool,
    pub both_plots: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let (controls, knobs) = if size.height >= 240.0 {
        (76.0, Some(34.0))
    } else if size.height >= 160.0 {
        (56.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let available_plot = (size.height - TOOLBAR_H - controls).max(0.0);
    let plot = if available_plot >= 76.0 {
        available_plot
    } else {
        0.0
    };
    Arrangement {
        plot,
        controls,
        knobs,
        primary_beside: advanced && size.width >= 820.0,
        sidechain_column: size.width >= 820.0 && plot >= 160.0,
        both_plots: size.width >= MEDIUM_W,
    }
}
pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - 190.0
}
fn signal_meter<'a>(label: &'static str, levels: MeterValues) -> Element<'a, Message> {
    let reading = |value| {
        let db = vibez_core::level::amplitude_to_db(value);
        if db.is_finite() {
            format!("{db:.1}")
        } else {
            "-inf".into()
        }
    };
    column![
        section_label(label),
        canvas(LevelMeter { levels })
            .width(METER_W)
            .height(Length::Fill),
        text(format!("L {}", reading(levels.peak_l)))
            .size(8)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None),
        text(format!("R {}", reading(levels.peak_r)))
            .size(8)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None)
    ]
    .spacing(2)
    .width(50.0)
    .height(Length::Fill)
    .into()
}
fn visual_switch<'a>(
    track: TrackId,
    effect: &UiEffect,
    panel: &CompressorPanel,
    accent: Color,
) -> Element<'a, Message> {
    let selected = panel.visual;
    row([
        ("Transfer", CompressorVisual::Transfer),
        ("History", CompressorVisual::History),
    ]
    .into_iter()
    .map(|(label, visual)| {
        button(text(label).size(9))
            .padding([1, 4])
            .on_press(Message::CompressorControl(CompressorMsg::Visual(
                track, effect.id, visual,
            )))
            .style(move |_, _| button::Style {
                text_color: if selected == visual {
                    accent
                } else {
                    th::text_dim()
                },
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: if selected == visual {
                        accent
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            })
            .into()
    }))
    .spacing(3)
    .into()
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
    let toolbar = container(
        row![
            preset,
            horizontal_space(),
            text(format!("GR {gr:.1} dB"))
                .size(9)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
            disclosure(track, effect, state.advanced)
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center),
    )
    .padding([0, 4])
    .height(TOOLBAR_H);
    let primary =
        || crate::widgets::compressor_controls::primary(track, effect, state, accent, layout.knobs);
    let advanced = || {
        crate::widgets::compressor_controls::advanced(
            track, effect, state, panel, accent, sources, buses, devices, input, output,
        )
    };
    let controls: Element<'a, Message> = if layout.primary_beside {
        row![container(primary()).width(370.0), advanced()]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .into()
    } else if state.advanced {
        advanced()
    } else {
        primary()
    };
    let footer = container(controls)
        .padding([0, 4])
        .height(layout.controls)
        .align_y(iced::alignment::Vertical::Center);
    let mut body = column![toolbar];
    if layout.plot > 0.0 {
        let header = |label| -> Element<'a, Message> {
            if layout.both_plots {
                section_label(label)
            } else {
                row![
                    section_label(label),
                    horizontal_space(),
                    visual_switch(track, effect, panel, accent)
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into()
            }
        };
        let transfer = || -> Element<'a, Message> {
            let title = header(if layout.both_plots {
                "Static transfer · manual internal detector"
            } else {
                "Manual internal transfer"
            });
            let title_height = if layout.both_plots {
                8.0 * 1.3 + 3.0
            } else {
                9.0 * 1.3 + 2.0 + 3.0
            };
            let compact_axes = layout.plot - 4.0 - title_height < 80.0;
            let mut content = column![title];
            if compact_axes {
                content = content.push(section_label("Output (dB) ↑ · Input (dB) →"));
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
                .spacing(3)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };
        let history = || -> Element<'a, Message> {
            column![
                header("Window peak wet GR · dB"),
                canvas(ReductionHistory {
                    values: panel.history().collect(),
                    accent
                })
                .width(Length::Fill)
                .height(Length::Fill)
            ]
            .spacing(3)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };
        let plots: Element<'a, Message> = if layout.both_plots {
            row![transfer(), history()]
                .spacing(8)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if panel.visual == CompressorVisual::Transfer {
            transfer()
        } else {
            history()
        };
        let mut visual = row![].spacing(8).height(Length::Fill);
        if layout.sidechain_column {
            visual = visual.push(
                container(crate::widgets::native_sidechain::view(
                    track, effect, sources, buses, devices, panel, accent, true,
                ))
                .width(128.0),
            );
        }
        if layout.plot >= 100.0 {
            visual = visual.push(signal_meter("Input", input));
        }
        visual = visual.push(plots);
        let gauge = column![
            section_label("Wet GR"),
            text(format!("{gr:.1} dB"))
                .size(9)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
            canvas(ReductionGauge {
                current,
                peak,
                accent
            })
            .width(50.0)
            .height(Length::Fill)
        ]
        .spacing(3)
        .width(54.0)
        .height(Length::Fill);
        visual = visual.push(gauge);
        if layout.plot >= 100.0 {
            visual = visual.push(signal_meter("Output", output));
        }
        body = body.push(container(visual).padding([2, 4]).height(layout.plot));
    }
    body.push(footer)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
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
                        TOOLBAR_H + layout.plot + layout.controls <= height + 0.001,
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
                    let caption = if layout.both_plots {
                        8.0 * 1.3 + 3.0
                    } else {
                        9.0 * 1.3 + 2.0 + 3.0
                    };
                    assert!(
                        layout.plot == 0.0 || layout.plot >= 4.0 + caption + 32.0,
                        "no miniature plots"
                    );
                    assert_eq!(layout.primary_beside, advanced && width >= 820.0);
                    assert!(!layout.sidechain_column || width >= 820.0 && layout.plot >= 150.0);
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
                                >= 3.0 * (8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0)
                                    + 9.0 * 1.3
                                    + 4.0
                                    + 2.0 * 8.0 * 1.3
                                    + 25.0
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
        let five_gaps = 5.0 * 5.0;
        let child_content = 3.0 * field + disconnect + warnings_and_raw_peak + five_gaps;
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
                let caption = if layout.both_plots {
                    8.0 * 1.3 + 3.0
                } else {
                    9.0 * 1.3 + 2.0 + 3.0
                };
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
}
