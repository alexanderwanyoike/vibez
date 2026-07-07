use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use vibez_core::effect::EffectType;

use crate::state::{UiEffect, UiTrack};
use crate::theme as th;
use crate::widgets::effect_knob::EffectKnobWidget;
use crate::widgets::fader::FaderWidget;
use crate::widgets::knob::KnobWidget;
use crate::widgets::vu_meter::VuMeterWidget;
use vibez_core::midi::TrackKind;

/// Render a single mixer channel strip for a track.
pub fn view_mixer_strip(track: &UiTrack) -> Element<'_, Message> {
    let track_color = th::track_color(track.color_index);

    // Track name + type icon
    let type_icon = match track.kind {
        TrackKind::Audio => icons::icon(icons::AUDIO_WAVEFORM)
            .size(10)
            .color(track_color),
        TrackKind::Instrument(_) | TrackKind::Midi => {
            icons::icon(icons::MUSIC).size(10).color(track_color)
        }
    };

    let name = text(&track.name)
        .size(12)
        .color(th::TEXT)
        .width(Length::Fill);

    let name_row = row![type_icon, name]
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Pan knob (bigger)
    let knob = KnobWidget::new(track.id, track.pan, track_color);
    let knob_canvas: Element<'_, Message> = canvas(knob)
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .into();

    let pan_label = text(format_pan(track.pan)).size(10).color(th::TEXT_DIM);

    // Fader (wider)
    let fader = FaderWidget::new(track.id, track.gain, track_color);
    let fader_canvas: Element<'_, Message> = canvas(fader)
        .width(Length::Fixed(32.0))
        .height(Length::Fill)
        .into();

    let gain_label = text(format_gain_db(track.gain))
        .size(11)
        .color(th::TEXT_DIM);

    // VU meter (wider)
    let meter = VuMeterWidget {
        peak_l: track.peak_l,
        peak_r: track.peak_r,
    };
    let meter_canvas: Element<'_, Message> = canvas(meter)
        .width(Length::Fixed(24.0))
        .height(Length::Fill)
        .into();

    // Mute button with filled background when active
    let mute_btn = {
        let label = text("M").size(11);
        if track.mute {
            button(label.color(th::BG_DARK))
                .on_press(Message::set_track_mute(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::MUTE_ACTIVE.into()),
                    text_color: th::BG_DARK,
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        } else {
            button(label.color(th::TEXT_DIM))
                .on_press(Message::set_track_mute(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
        }
    };

    // Solo button with filled background when active
    let solo_btn = {
        let label = text("S").size(11);
        if track.solo {
            button(label.color(th::BG_DARK))
                .on_press(Message::set_track_solo(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::SOLO_ACTIVE.into()),
                    text_color: th::BG_DARK,
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        } else {
            button(label.color(th::TEXT_DIM))
                .on_press(Message::set_track_solo(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
        }
    };

    let mute_solo_row = row![mute_btn, solo_btn].spacing(4);

    // Fader + meter side by side
    let fader_meter = row![fader_canvas, meter_canvas]
        .spacing(2)
        .height(Length::Fill);

    let eq_section = view_strip_eq(track);

    let strip = column![
        name_row,
        knob_canvas,
        pan_label,
        eq_section,
        fader_meter,
        gain_label,
        mute_solo_row,
    ]
    .spacing(4)
    .padding(8)
    .width(Length::Fixed(108.0))
    .height(Length::Fill)
    .align_x(iced::Alignment::Center);

    container(strip)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// SSL-style channel EQ: the strip renders the track's first
/// built-in EQ effect as four color-coded bands (HF red, HMF green,
/// LMF blue, LF brown, like the console), with bell toggles on the
/// outer bands and an IN (bypass) button. The EQ is an ordinary
/// device on the chain, so automation and persistence come free.
fn view_strip_eq(track: &UiTrack) -> Element<'_, Message> {
    let eq = track
        .effects
        .iter()
        .find(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none());

    let Some(eq) = eq else {
        return container(
            button(text("+ EQ").size(10).color(th::TEXT_DIM))
                .on_press(Message::add_effect(track.id, EffectType::Eq))
                .padding([2, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => Some(th::BG_ELEVATED.into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT_DIM,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }
                }),
        )
        .padding([4, 0])
        .into();
    };

    // Console band colors, muted for the theme.
    const HF: iced::Color = iced::Color::from_rgb(0.76, 0.33, 0.30);
    const HMF: iced::Color = iced::Color::from_rgb(0.33, 0.62, 0.38);
    const LMF: iced::Color = iced::Color::from_rgb(0.36, 0.48, 0.72);
    const LF: iced::Color = iced::Color::from_rgb(0.65, 0.46, 0.28);

    let mut bands = column![].spacing(2).align_x(iced::Alignment::Center);

    // Header: EQ label + IN (bypass) toggle.
    let in_btn = {
        let active = !eq.bypass;
        let label = text("IN").size(8);
        button(if active {
            label.color(th::BG_DARK)
        } else {
            label.color(th::TEXT_DIM)
        })
        .on_press(Message::toggle_effect_bypass(track.id, eq.id))
        .padding([1, 5])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(if active {
                th::ACCENT.into()
            } else {
                th::BG_ELEVATED.into()
            }),
            text_color: if active { th::BG_DARK } else { th::TEXT_DIM },
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
    };
    bands = bands.push(
        row![text("EQ").size(9).color(th::TEXT_DIM), in_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center),
    );

    // (gain idx, freq idx, q/bell idx, bell?, color, label)
    let rows: [(usize, usize, usize, bool, iced::Color, &str); 4] = [
        (9, 10, 11, true, HF, "HF"),
        (6, 7, 8, false, HMF, "HMF"),
        (3, 4, 5, false, LMF, "LMF"),
        (0, 1, 2, true, LF, "LF"),
    ];
    for (gain_i, freq_i, third_i, is_bell_toggle, color, label) in rows {
        bands = bands.push(view_eq_band(
            track.id,
            eq,
            gain_i,
            freq_i,
            third_i,
            is_bell_toggle,
            color,
            label,
        ));
    }

    container(bands).padding([4, 0]).width(Length::Fill).into()
}

#[allow(clippy::too_many_arguments)]
fn view_eq_band<'a>(
    track_id: vibez_core::id::TrackId,
    eq: &'a UiEffect,
    gain_i: usize,
    freq_i: usize,
    third_i: usize,
    bell_toggle: bool,
    color: iced::Color,
    label: &'static str,
) -> Element<'a, Message> {
    let knob = |i: usize| -> Element<'a, Message> {
        let d = &eq.descriptors[i];
        let w = EffectKnobWidget::new(
            track_id,
            eq.id,
            i,
            eq.params.get(i).copied().unwrap_or(d.default),
            d.min,
            d.max,
            d.default,
            color,
        );
        canvas(w)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .into()
    };

    let third: Element<'a, Message> = if bell_toggle {
        let bell_on = eq.params.get(third_i).copied().unwrap_or(0.0) >= 0.5;
        button(
            text("B")
                .size(8)
                .color(if bell_on { th::BG_DARK } else { th::TEXT_DIM }),
        )
        .on_press(Message::set_effect_param(
            track_id,
            eq.id,
            third_i,
            if bell_on { 0.0 } else { 1.0 },
        ))
        .padding([2, 4])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(if bell_on {
                color.into()
            } else {
                th::BG_ELEVATED.into()
            }),
            text_color: if bell_on { th::BG_DARK } else { th::TEXT_DIM },
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        knob(third_i)
    };

    column![
        text(label).size(8).color(color),
        row![knob(gain_i), knob(freq_i), third]
            .spacing(3)
            .align_y(iced::Alignment::Center),
    ]
    .spacing(1)
    .align_x(iced::Alignment::Center)
    .into()
}

fn format_pan(pan: f32) -> String {
    if (pan - 0.5).abs() < 0.01 {
        "C".to_string()
    } else if pan < 0.5 {
        format!("L{:.0}", (0.5 - pan) * 200.0)
    } else {
        format!("R{:.0}", (pan - 0.5) * 200.0)
    }
}

fn format_gain_db(gain: f32) -> String {
    if gain <= 0.001 {
        "-inf".to_string()
    } else {
        let db = 20.0 * gain.log10();
        format!("{db:+.1}")
    }
}
