//! Responsive native EQ response, selected-band controls and genuine analyser choices.

use crate::widgets::{
    device_chrome::section_label,
    device_controls::{disclosure, parameter, selector},
    eq_display::EqDisplayWidget,
    level_meter::{LevelMeter, WIDTH as METER_W},
};
use crate::{
    domains::{
        device_controls::DeviceControlState,
        eq::{EqMsg, EqPanel},
    },
    message::Message,
    state::UiEffect,
    theme as th,
};
use iced::{
    widget::{button, canvas, column, container, horizontal_space, pick_list, row, text},
    Color, Element, Length, Size,
};
use vibez_core::id::TrackId;
use vibez_dsp::eq::param;
use vibez_engine::{effect_spectrum::EffectSpectrumPosition, metering::MeterValues};

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 620.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;
const BAND_NAMES: [&str; 4] = ["LF", "LMF", "HMF", "HF"];

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}
pub fn preset_bar_width(size: Size) -> f32 {
    size.width - 106.0
}

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub graph: f32,
    pub controls: f32,
    pub knobs: Option<f32>,
    pub all_bands: bool,
    pub advanced: bool,
    pub inline_primary: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let short = size.height < 114.0;
    let (controls, knobs) = if short {
        (36.0, None)
    } else if size.height >= 220.0 {
        (90.0, Some(34.0))
    } else if size.height >= 150.0 {
        (56.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let graph = if short {
        0.0
    } else {
        (size.height - TOOLBAR_H - controls).max(0.0)
    };
    Arrangement {
        graph,
        controls,
        knobs,
        all_bands: !advanced && size.width >= 820.0 && size.height >= 220.0,
        advanced,
        inline_primary: advanced && graph < 160.0 && size.width >= 820.0,
    }
}

fn value(effect: &UiEffect, index: usize) -> f32 {
    effect
        .params
        .get(index)
        .copied()
        .unwrap_or(effect.descriptors[index].default)
}

fn choice<'a>(
    label: &'static str,
    active: bool,
    accent: Color,
    message: Message,
) -> Element<'a, Message> {
    button(text(label).size(9))
        .on_press(message)
        .padding([2, 4])
        .style(move |_, status| button::Style {
            text_color: if active { accent } else { th::text_dim() },
            background: Some(
                if active || matches!(status, button::Status::Hovered | button::Status::Pressed) {
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

fn toggle<'a>(
    track: TrackId,
    effect: &UiEffect,
    index: usize,
    label: &'static str,
    accent: Color,
) -> Element<'a, Message> {
    let on = value(effect, index) >= 0.5;
    choice(
        label,
        on,
        accent,
        Message::set_effect_param(track, effect.id, index, if on { 0.0 } else { 1.0 }),
    )
}

fn band_choice<'a>(
    track: TrackId,
    effect: &UiEffect,
    panel: &EqPanel,
    accent: Color,
) -> Element<'a, Message> {
    let id = effect.id;
    column![
        section_label("Band"),
        pick_list(
            BAND_NAMES,
            Some(BAND_NAMES[panel.selected_band]),
            move |name| Message::EqControl(EqMsg::SelectBand(
                track,
                id,
                BAND_NAMES
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap_or(0)
            ))
        )
        .padding([1, 3])
        .text_size(10)
        .width(Length::Fill)
        .style(move |_, _| pick_list::Style {
            text_color: accent,
            placeholder_color: th::text_dim(),
            handle_color: th::text_dim(),
            background: th::bg_dark().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into()
            }
        })
        .menu_style(move |_| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(),
            text_color: th::text(),
            selected_text_color: accent,
            selected_background: th::bg_hover().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into()
            }
        })
    ]
    .spacing(3)
    .width(Length::Fixed(60.0))
    .into()
}

fn band_controls<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    band: usize,
    accent: Color,
    knobs: Option<f32>,
) -> Element<'a, Message> {
    let shape = if band == 0 || band == 3 {
        selector(
            track,
            effect,
            param::SHAPE[band],
            "Shape",
            &["Shelf", "Bell"],
            accent,
        )
    } else {
        parameter(track, effect, state, param::SHAPE[band], accent, knobs)
    };
    row![
        parameter(track, effect, state, param::FREQUENCY[band], accent, knobs),
        parameter(track, effect, state, param::GAIN[band], accent, knobs),
        shape,
    ]
    .spacing(6)
    .width(Length::Fill)
    .align_y(iced::Alignment::Center)
    .into()
}

fn band_switches<'a>(
    track: TrackId,
    effect: &UiEffect,
    band: usize,
    accent: Color,
) -> Element<'a, Message> {
    let audition = value(effect, param::AUDITION).round() as usize == band + 1;
    column![
        toggle(track, effect, param::ENABLE[band], "On", accent),
        choice(
            "Listen",
            audition,
            accent,
            Message::set_effect_param(
                track,
                effect.id,
                param::AUDITION,
                if audition { 0.0 } else { (band + 1) as f32 }
            )
        ),
    ]
    .spacing(2)
    .align_x(iced::Alignment::Center)
    .into()
}

fn analyser_controls<'a>(
    track: TrackId,
    effect: &UiEffect,
    panel: &EqPanel,
    accent: Color,
    active: bool,
) -> Element<'a, Message> {
    row![
        choice(
            if active { "Tapped" } else { "Analyse" },
            active,
            accent,
            Message::EqControl(EqMsg::Focus(track, effect.id))
        ),
        choice(
            "Pre",
            !panel.post,
            accent,
            Message::EqControl(EqMsg::Source(
                track,
                effect.id,
                EffectSpectrumPosition::Input
            ))
        ),
        choice(
            "Post",
            panel.post,
            accent,
            Message::EqControl(EqMsg::Source(
                track,
                effect.id,
                EffectSpectrumPosition::Output
            ))
        ),
        choice(
            "Freeze",
            panel.frozen,
            accent,
            Message::EqControl(EqMsg::Freeze(track, effect.id))
        ),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center)
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    panel: &'a EqPanel,
    analyser_active: bool,
    preset: Element<'a, Message>,
    output: MeterValues,
    sample_rate: f32,
    size: Size,
) -> Element<'a, Message> {
    let layout = arrange(size, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let selected = panel.selected_band;
    let primary = |knobs| -> Element<'a, Message> {
        row![
            band_choice(track, effect, panel, accent),
            band_controls(track, effect, state, selected, accent, knobs),
            band_switches(track, effect, selected, accent)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    };
    let advanced = |knobs| -> Element<'a, Message> {
        row![
            toggle(track, effect, param::HP_ENABLED, "HP", accent),
            parameter(track, effect, state, param::HP_FREQUENCY, accent, knobs),
            toggle(track, effect, param::LP_ENABLED, "LP", accent),
            parameter(track, effect, state, param::LP_FREQUENCY, accent, knobs),
            parameter(track, effect, state, param::OUTPUT, accent, knobs),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    };
    let toolbar = row![
        preset,
        horizontal_space(),
        disclosure(track, effect, state.advanced)
    ]
    .align_y(iced::Alignment::Center)
    .height(TOOLBAR_H);
    let controls: Element<'a, Message> = if layout.inline_primary {
        row![
            primary(None),
            advanced(None),
            analyser_controls(track, effect, panel, accent, analyser_active)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.advanced && layout.graph < 72.0 {
        row![
            advanced(None),
            analyser_controls(track, effect, panel, accent, analyser_active)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.all_bands {
        let mut bands = row![].spacing(10);
        for (band, label) in BAND_NAMES.into_iter().enumerate() {
            let heading = row![
                choice(
                    label,
                    selected == band,
                    accent,
                    Message::EqControl(EqMsg::SelectBand(track, effect.id, band))
                ),
                horizontal_space(),
                toggle(track, effect, param::ENABLE[band], "On", accent),
            ]
            .align_y(iced::Alignment::Center);
            bands = bands.push(
                column![
                    heading,
                    band_controls(track, effect, state, band, accent, layout.knobs)
                ]
                .spacing(3)
                .width(Length::Fill),
            );
        }
        row![
            bands,
            container(parameter(
                track,
                effect,
                state,
                param::OUTPUT,
                accent,
                layout.knobs
            ))
            .width(70.0)
        ]
        .spacing(12)
        .into()
    } else if layout.advanced {
        advanced(layout.knobs)
    } else {
        primary(layout.knobs)
    };
    let footer = container(controls)
        .padding([0, 4])
        .height(layout.controls)
        .center_y(Length::Fill);
    let mut body = column![toolbar];
    if layout.graph >= 32.0 {
        let graph = canvas(EqDisplayWidget {
            track_id: track,
            effect_id: effect.id,
            params: effect.params.clone(),
            descriptors: effect.descriptors,
            bypass: effect.bypass,
            accent,
            selected_band: selected,
            sample_rate,
            spectrum: panel.spectrum.bins.clone(),
            spectrum_active: panel.spectrum.active,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let caption = section_label("Static response · manual values");
        let plot_header: Element<'a, Message> = if layout.graph >= 72.0 {
            row![
                caption,
                horizontal_space(),
                analyser_controls(track, effect, panel, accent, analyser_active)
            ]
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            caption
        };
        let plot: Element<'a, Message> = column![plot_header, graph]
            .spacing(3)
            .height(Length::Fill)
            .into();
        let meter_canvas = canvas(LevelMeter { levels: output })
            .width(METER_W)
            .height(Length::Fill);
        let meter: Element<'a, Message> = if layout.graph >= 100.0 {
            let reading = |peak| {
                let db = vibez_core::level::amplitude_to_db(peak);
                if db.is_finite() {
                    format!("{db:.1}")
                } else {
                    "-inf".into()
                }
            };
            column![
                section_label("Out"),
                meter_canvas,
                text(format!("L {}", reading(output.peak_l)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
                text(format!("R {}", reading(output.peak_r)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(2)
            .width(METER_W)
            .height(Length::Fill)
            .into()
        } else {
            meter_canvas.into()
        };
        let visual: Element<'a, Message> = if layout.graph >= 160.0 && size.width >= MEDIUM_W {
            let shape = if selected == 0 || selected == 3 {
                selector(
                    track,
                    effect,
                    param::SHAPE[selected],
                    "Shape",
                    &["Shelf", "Bell"],
                    accent,
                )
            } else {
                parameter(track, effect, state, param::SHAPE[selected], accent, None)
            };
            let editor = column![
                row![
                    band_choice(track, effect, panel, accent),
                    horizontal_space(),
                    band_switches(track, effect, selected, accent)
                ]
                .align_y(iced::Alignment::Center),
                parameter(
                    track,
                    effect,
                    state,
                    param::FREQUENCY[selected],
                    accent,
                    None
                ),
                parameter(track, effect, state, param::GAIN[selected], accent, None),
                shape,
            ]
            .spacing(6);
            row![plot, container(editor).width(160.0), meter]
                .height(Length::Fill)
                .spacing(10)
                .into()
        } else {
            row![plot, meter].height(Length::Fill).spacing(8).into()
        };
        body = body.push(container(visual).height(layout.graph).padding([2, 4]));
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
    fn every_measured_tier_fits_actual_minimum_body_without_internal_scrolling() {
        for width in [430.0, 620.0, 900.0] {
            for height in [
                64.0, 72.0, 90.0, 108.0, 113.0, 114.0, 130.0, 160.0, 240.0, 400.0,
            ] {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    assert!(
                        TOOLBAR_H + layout.controls + layout.graph <= height + 0.001,
                        "{width}x{height} advanced={advanced}"
                    );
                    assert!(layout.controls >= if layout.all_bands { 90.0 } else { 36.0 });
                    let numeric = 8.0 * 1.3
                        + 3.0
                        + 10.0 * 1.3
                        + 2.0
                        + layout.knobs.map_or(0.0, |knob| knob + 3.0);
                    let buttons = 2.0 * (9.0 * 1.3 + 4.0) + 2.0;
                    let children = if layout.all_bands {
                        numeric + 3.0 + 9.0 * 1.3 + 4.0
                    } else {
                        numeric.max(buttons)
                    };
                    assert!(
                        layout.controls >= children,
                        "child controls clip at {width}x{height}"
                    );
                    let plot_header = if layout.graph >= 72.0 {
                        9.0 * 1.3 + 4.0 + 3.0
                    } else {
                        8.0 * 1.3 + 3.0
                    };
                    assert!(layout.graph == 0.0 || layout.graph >= 4.0 + plot_header + 32.0);
                    assert_eq!(
                        layout.inline_primary,
                        advanced && layout.graph < 160.0 && width >= 820.0
                    );
                }
            }
        }
    }
    #[test]
    fn width_uses_chain_remainder_and_is_bounded() {
        assert_eq!(card_width(80.0), 430.0);
        assert_eq!(card_width(710.0), 710.0);
        assert_eq!(card_width(1800.0), 900.0);
    }

    #[test]
    fn wide_advanced_keeps_main_readouts_when_the_plot_cannot_fit_a_side_editor() {
        for height in [130.0, 160.0, 240.0] {
            assert!(
                arrange(Size::new(900.0, height), true).inline_primary,
                "main readouts hidden at height {height}"
            );
        }
    }
}
