//! Filter response, musical controls and modulation arranged to the measured body.

use crate::widgets::{
    device_chrome::section_label,
    device_controls::{disclosure, parameter, selector},
    filter_response::FilterResponse,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{domains::device_controls::DeviceControlState, message::Message, state::UiEffect};
use iced::{
    widget::{canvas, column, container, row, text, Space},
    Color, Element, Length, Size,
};
use vibez_core::id::TrackId;
use vibez_dsp::filter::param;
use vibez_engine::metering::MeterValues;

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 620.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub controls: f32,
    pub knobs: Option<f32>,
    pub plot: f32,
    pub beside: bool,
    pub inline_primary: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let (controls, knobs) = if advanced && size.height < 100.0 {
        (0.0, None)
    } else if size.height >= 220.0 {
        (74.0, Some(38.0))
    } else if size.height >= 140.0 {
        (54.0, Some(22.0))
    } else {
        (32.0, None)
    };
    Arrangement {
        controls,
        knobs,
        plot: (size.height - TOOLBAR_H - controls).max(0.0),
        beside: advanced && size.width >= MEDIUM_W,
        inline_primary: advanced && size.height < 100.0 && size.width >= 820.0,
    }
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - 106.0
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
    let layout = arrange(size, state.advanced);
    let accent = if effect.bypass {
        crate::theme::text_muted()
    } else {
        accent
    };
    let primary = || {
        row![
            selector(
                track,
                effect,
                param::MODE,
                "Type",
                &["LP", "HP", "BP", "Notch"],
                accent
            ),
            selector(
                track,
                effect,
                param::SLOPE,
                "Slope",
                &["12 dB", "24 dB"],
                accent
            ),
            parameter(track, effect, state, param::CUTOFF, accent, layout.knobs),
            parameter(track, effect, state, param::RESONANCE, accent, layout.knobs),
            parameter(track, effect, state, param::DRIVE, accent, layout.knobs),
            parameter(track, effect, state, param::MIX, accent, layout.knobs),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
    };
    let response_canvas = canvas(FilterResponse {
        track,
        effect,
        sample_rate,
        accent,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let plot: Element<'a, Message> = if layout.plot < 42.0 {
        iced::widget::tooltip(
            response_canvas,
            text("Linear response · drive and modulation excluded").size(9),
            iced::widget::tooltip::Position::Top,
        )
        .into()
    } else {
        column![
            section_label("Linear response · drive and modulation excluded"),
            response_canvas,
        ]
        .spacing(3)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };
    let visual: Element<'a, Message> = if layout.inline_primary {
        row![
            primary(),
            container(modulation(track, effect, state, accent, false)).width(360.0)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
    } else if state.advanced && !layout.beside {
        modulation(track, effect, state, accent, false)
    } else {
        let mut meter = column![
            section_label("Output"),
            canvas(LevelMeter { levels: output })
                .width(METER_W)
                .height(Length::Fill)
        ]
        .spacing(3)
        .width(METER_W);
        if layout.plot >= 72.0 {
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
                        .wrapping(iced::widget::text::Wrapping::None)
                        .color(crate::theme::text()),
                );
            }
        }
        let meter: Element<'a, Message> = if layout.plot < 42.0 {
            let reading = |peak| {
                let db = vibez_core::level::amplitude_to_db(peak);
                if db.is_finite() {
                    format!("{db:.1}")
                } else {
                    "-inf".into()
                }
            };
            text(format!(
                "L {}  R {} dBFS",
                reading(output.peak_l),
                reading(output.peak_r)
            ))
            .size(8)
            .wrapping(iced::widget::text::Wrapping::None)
            .color(crate::theme::text())
            .into()
        } else {
            meter.into()
        };
        let response = row![plot, meter]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .height(Length::Fill);
        if layout.beside {
            row![
                response.width(Length::Fill),
                container(modulation(
                    track,
                    effect,
                    state,
                    accent,
                    layout.plot >= 80.0
                ))
                .width(if layout.plot >= 80.0 { 260.0 } else { 360.0 })
            ]
            .spacing(12)
            .into()
        } else {
            response.into()
        }
    };
    let mut body = column![
        container(
            row![
                preset,
                Space::with_width(Length::Fill),
                disclosure(track, effect, state.advanced)
            ]
            .align_y(iced::Alignment::Center)
        )
        .padding([0, 8])
        .height(TOOLBAR_H),
        container(visual).padding([2, 8]).height(layout.plot),
    ];
    if layout.controls > 0.0 {
        body = body.push(container(primary()).padding([2, 8]).height(layout.controls));
    }
    body.height(Length::Fill).width(Length::Fill).into()
}

fn modulation<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    stacked: bool,
) -> Element<'a, Message> {
    let source = selector(
        track,
        effect,
        param::MOD_SOURCE,
        "Modulation",
        &["Off", "LFO", "Envelope"],
        accent,
    );
    let depth = parameter(track, effect, state, param::DEPTH, accent, None);
    let value = effect.params.get(param::MOD_SOURCE).copied().unwrap_or(0.0);
    let rest = if value == 2.0 {
        row![
            parameter(track, effect, state, param::ENV_ATTACK, accent, None),
            parameter(track, effect, state, param::ENV_RELEASE, accent, None)
        ]
    } else {
        row![
            parameter(track, effect, state, param::RATE, accent, None),
            selector(
                track,
                effect,
                param::SYNC,
                "Sync",
                &["Free", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8D"],
                accent
            ),
            parameter(track, effect, state, param::STEREO_PHASE, accent, None)
        ]
    }
    .spacing(8)
    .align_y(iced::Alignment::Center);
    let controls: Element<'a, Message> = if stacked {
        column![row![source, depth].spacing(8), rest]
            .spacing(8)
            .into()
    } else {
        row![
            source,
            depth,
            rest.width(Length::FillPortion(if value == 2.0 { 2 } else { 3 }))
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    };
    container(controls)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_tiers_fit_the_measured_height_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, MAX_W] {
            for height in [64.0, 72.0, 90.0, 130.0, 160.0, 240.0, 400.0] {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    assert!(TOOLBAR_H + layout.controls + layout.plot <= height + 0.01);
                    assert!(layout.plot >= 14.0);
                    if advanced {
                        let content = if layout.beside && layout.plot >= 80.0 {
                            64.0
                        } else {
                            28.0
                        };
                        assert!(
                            layout.plot - 4.0 >= content,
                            "modulation contents do not fit {width}x{height}"
                        );
                    }
                    if advanced && width < MEDIUM_W {
                        assert!(!layout.beside);
                    }
                }
            }
        }
    }
}
