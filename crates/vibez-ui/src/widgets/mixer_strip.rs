use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use vibez_core::effect::EffectType;

use crate::state::{ProjectTrack, UiEffect};
use crate::theme as th;
use crate::widgets::effect_knob::EffectKnobWidget;
use crate::widgets::fader::FaderWidget;
use crate::widgets::knob::KnobWidget;
use crate::widgets::track_header::view_editable_channel_name;
use crate::widgets::vu_meter::VuMeterWidget;
use vibez_core::midi::TrackKind;

/// What kind of channel a mixer strip renders. Tracks get the full
/// set plus send knobs; buses trade sends/solo for a RETURN badge;
/// the master drops everything that makes no sense on a sum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StripRole {
    Track,
    Bus,
    Master,
}

/// Per-render inputs that are not part of the project-owned channel itself.
pub struct MixerStripView<'a> {
    pub selected: bool,
    pub editing_name: bool,
    pub edit_text: &'a str,
    pub playhead_beat: f64,
    pub automation: &'a [vibez_core::automation::AutomationLane],
}

/// Render a single mixer channel strip. `buses` drives the send
/// knob section on regular track strips.
pub fn view_mixer_strip<'a>(
    track: &'a ProjectTrack,
    role: StripRole,
    buses: &'a [ProjectTrack],
    view: MixerStripView<'a>,
) -> Element<'a, Message> {
    let is_master = role == StripRole::Master;
    let track_color = if is_master {
        th::accent()
    } else {
        th::track_color(track.color_index)
    };

    // Track name + type icon
    let type_icon = match role {
        StripRole::Master => icons::icon(icons::VOLUME_2).size(10).color(track_color),
        StripRole::Bus => icons::icon(icons::VOLUME_2).size(10).color(track_color),
        StripRole::Track => match track.kind {
            TrackKind::Audio => icons::icon(icons::AUDIO_WAVEFORM)
                .size(10)
                .color(track_color),
            TrackKind::Instrument(_) | TrackKind::Midi => {
                icons::icon(icons::MUSIC).size(10).color(track_color)
            }
        },
    };

    let name: Element<'a, Message> = if is_master {
        text(&track.name)
            .size(12)
            .color(th::text())
            .width(Length::Fill)
            .into()
    } else {
        view_editable_channel_name(track, view.editing_name, view.edit_text, 12, th::text())
    };

    // Tracks are deletable straight from the strip; the master is
    // not, and buses carry their remove control on the RETURN badge.
    let delete_el: Element<'a, Message> = if role == StripRole::Track {
        button(icons::icon(icons::TRASH_2).size(9).color(th::text_dim()))
            .on_press(Message::remove_track(track.id))
            .padding([1, 3])
            .style(|_theme: &Theme, status| {
                let tc = match status {
                    button::Status::Hovered | button::Status::Pressed => th::danger(),
                    _ => th::text_dim(),
                };
                button::Style {
                    background: None,
                    text_color: tc,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            })
            .into()
    } else {
        text("").size(9).into()
    };

    let name_row = row![type_icon, name, delete_el]
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Pan knob (bigger)
    let knob = KnobWidget::new(track.id, track.pan, track_color);
    let knob_canvas: Element<'_, Message> = canvas(knob)
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .into();

    let pan_label = text(format_pan(track.pan)).size(10).color(th::text_dim());

    // Fader (wider)
    let fader = FaderWidget::new(track.id, track.gain, track_color);
    let fader_canvas: Element<'_, Message> = canvas(fader)
        .width(Length::Fixed(32.0))
        .height(Length::Fill)
        .into();

    let gain_label = text(format_gain_db(track.gain))
        .size(11)
        .color(th::text_dim());

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
            button(label.color(th::bg_dark()))
                .on_press(Message::set_track_mute(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::mute_active().into()),
                    text_color: th::bg_dark(),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        } else {
            button(label.color(th::text_dim()))
                .on_press(Message::set_track_mute(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::text_dim(),
                    border: iced::Border {
                        color: th::border(),
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
            button(label.color(th::bg_dark()))
                .on_press(Message::set_track_solo(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::solo_active().into()),
                    text_color: th::bg_dark(),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        } else {
            button(label.color(th::text_dim()))
                .on_press(Message::set_track_solo(track.id))
                .padding([4, 8])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::text_dim(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
        }
    };

    // Fader + meter side by side
    let fader_meter = row![fader_canvas, meter_canvas]
        .spacing(2)
        .height(Length::Fill);

    let eq_section = view_strip_eq(track);

    let strip = if is_master {
        // No pan, mute, or solo on the sum: a MASTER tag keeps the
        // strip's vertical rhythm instead.
        let badge = container(text("MASTER").size(8).color(th::accent()))
            .padding([3, 8])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_elevated().into()),
                border: iced::Border {
                    color: th::accent_dim(),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
        column![name_row, eq_section, badge, fader_meter, gain_label]
    } else if role == StripRole::Bus {
        // Return channel: RETURN badge + remove control in place of
        // sends; balance pan, mute, and solo still apply.
        let badge = container(text("RETURN").size(8).color(track_color))
            .padding([3, 6])
            .style(move |_theme: &Theme| container::Style {
                background: Some(th::bg_elevated().into()),
                border: iced::Border {
                    color: th::darken(track_color, 0.5),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
        let remove_btn = button(icons::icon(icons::X).size(9).color(th::text_dim()))
            .on_press(Message::remove_bus(track.id))
            .padding([2, 5])
            .style(|_theme: &Theme, status| {
                let (bg, tc) = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        (Some(th::bg_hover().into()), th::danger())
                    }
                    _ => (None, th::text_dim()),
                };
                button::Style {
                    background: bg,
                    text_color: tc,
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
        let badge_row = row![badge, remove_btn]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        column![
            name_row,
            eq_section,
            badge_row,
            knob_canvas,
            pan_label,
            fader_meter,
            gain_label,
            row![mute_btn, solo_btn].spacing(4),
        ]
    } else {
        let mut col = column![name_row, eq_section];
        if !buses.is_empty() {
            col = col.push(view_sends(
                track,
                buses,
                view.playhead_beat,
                view.automation,
            ));
        }
        col.push(knob_canvas)
            .push(pan_label)
            .push(fader_meter)
            .push(gain_label)
            .push(row![mute_btn, solo_btn].spacing(4))
    }
    .spacing(4)
    .padding(8)
    .width(Length::Fixed(94.0))
    .height(Length::Fill)
    .align_x(iced::Alignment::Center);

    let body = container(strip)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: if view.selected {
                    th::accent()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

    // Clicking anywhere on the strip chrome selects the track, so
    // the detail panel follows the mixer like it follows the
    // arrangement headers. Knobs, faders, and buttons still win
    // their own clicks.
    iced::widget::mouse_area(body)
        .on_press(Message::select_track(track.id))
        .into()
}

/// Send section on a track strip: one knob per bus, bus-colored,
/// wrapped three to a row under a SENDS header.
fn view_sends<'a>(
    track: &'a ProjectTrack,
    buses: &'a [ProjectTrack],
    playhead_beat: f64,
    automation: &'a [vibez_core::automation::AutomationLane],
) -> Element<'a, Message> {
    let mut section = column![text("SENDS").size(7).color(th::text_muted())]
        .spacing(2)
        .align_x(iced::Alignment::Center);
    for chunk in buses.chunks(3) {
        let mut knobs = row![].spacing(4).align_y(iced::Alignment::Center);
        for bus in chunk {
            let amount = effective_send_amount(track, automation, bus.id, playhead_beat);
            let letter = bus.name.chars().next().unwrap_or('?');
            let knob = EffectKnobWidget::for_send(
                track.id,
                bus.id,
                amount,
                th::track_color(bus.color_index),
            );
            let knob_canvas: Element<'a, Message> = canvas(knob)
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0))
                .into();
            knobs = knobs.push(
                column![
                    knob_canvas,
                    text(letter.to_string()).size(7).color(th::text_dim())
                ]
                .spacing(0)
                .align_x(iced::Alignment::Center),
            );
        }
        section = section.push(knobs);
    }
    container(section).padding([2, 0]).into()
}

fn effective_send_amount(
    track: &ProjectTrack,
    automation: &[vibez_core::automation::AutomationLane],
    bus_id: vibez_core::id::TrackId,
    playhead_beat: f64,
) -> f32 {
    automation
        .iter()
        .find_map(|lane| match lane.target {
            vibez_core::automation::AutomationTarget::Send { bus_id: target }
                if target == bus_id =>
            {
                lane.value_at(playhead_beat)
            }
            _ => None,
        })
        .or_else(|| {
            track
                .sends
                .iter()
                .find(|(target, _)| *target == bus_id)
                .map(|(_, amount)| *amount)
        })
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

/// SSL-style channel EQ: the strip renders the track's first
/// built-in EQ effect as four color-coded bands (HF red, HMF green,
/// LMF blue, LF brown, like the console), with bell toggles on the
/// outer bands and an IN (bypass) button. The EQ is an ordinary
/// device on the chain, so automation and persistence come free.
fn view_strip_eq(track: &ProjectTrack) -> Element<'_, Message> {
    let eq = track
        .effects
        .iter()
        .find(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none());

    let Some(eq) = eq else {
        return container(
            button(text("+ EQ").size(10).color(th::text_dim()))
                .on_press(Message::add_effect(track.id, EffectType::Eq))
                .padding([2, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text_dim(),
                        border: iced::Border {
                            color: th::border(),
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

    // Console band colors, from the current theme.
    let hf = th::eq_hf();
    let hmf = th::eq_hmf();
    let lmf = th::eq_lmf();
    let lf = th::eq_lf();

    let mut bands = column![].spacing(2).align_x(iced::Alignment::Center);

    // Header: EQ label + IN (bypass) toggle.
    let in_btn = {
        let active = !eq.bypass;
        let label = text("IN").size(8);
        button(if active {
            label.color(th::bg_dark())
        } else {
            label.color(th::text_dim())
        })
        .on_press(Message::toggle_effect_bypass(track.id, eq.id))
        .padding([1, 5])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(if active {
                th::accent().into()
            } else {
                th::bg_elevated().into()
            }),
            text_color: if active {
                th::bg_dark()
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
    };
    bands = bands.push(
        row![text("EQ").size(9).color(th::text_dim()), in_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center),
    );

    // (gain idx, freq idx, q/bell idx, bell?, color, label)
    let rows: [(usize, usize, usize, bool, iced::Color, &str); 4] = [
        (9, 10, 11, true, hf, "HF"),
        (6, 7, 8, false, hmf, "HMF"),
        (3, 4, 5, false, lmf, "LMF"),
        (0, 1, 2, true, lf, "LF"),
    ];
    for (i, (gain_i, freq_i, third_i, is_bell_toggle, color, label)) in rows.into_iter().enumerate()
    {
        if i > 0 {
            bands = bands.push(
                container(text(""))
                    .width(Length::Fixed(74.0))
                    .height(Length::Fixed(1.0))
                    .style(|_theme: &Theme| container::Style {
                        background: Some(
                            iced::Color {
                                a: 0.5,
                                ..th::border()
                            }
                            .into(),
                        ),
                        ..Default::default()
                    }),
            );
        }
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
    let knob = |i: usize, size: f32| -> Element<'a, Message> {
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
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into()
    };

    // Console stagger: the gain knob rides high on the left, the
    // frequency knob sits low on the right, Q (or the bell switch)
    // tucks under the gain. The eye zig-zags down the strip.
    let dim = iced::Color {
        a: 0.75,
        ..th::text_dim()
    };
    let gain_col = column![
        row![
            text(label).size(8).color(color),
            text("dB").size(7).color(dim)
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center),
        knob(gain_i, 26.0),
    ]
    .spacing(1)
    .align_x(iced::Alignment::Center);

    let third_el: Element<'a, Message> = if bell_toggle {
        let bell_on = eq.params.get(third_i).copied().unwrap_or(0.0) >= 0.5;
        button(text("BELL").size(6).color(if bell_on {
            th::bg_dark()
        } else {
            th::text_dim()
        }))
        .on_press(Message::set_effect_param(
            track_id,
            eq.id,
            third_i,
            if bell_on { 0.0 } else { 1.0 },
        ))
        .padding([3, 6])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(if bell_on {
                color.into()
            } else {
                th::bg_elevated().into()
            }),
            text_color: if bell_on {
                th::bg_dark()
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        column![text("Q").size(7).color(dim), knob(third_i, 22.0)]
            .spacing(1)
            .align_x(iced::Alignment::Center)
            .into()
    };

    let freq_col = column![
        text(if freq_i == 10 || freq_i == 7 {
            "kHz"
        } else {
            "Hz"
        })
        .size(7)
        .color(dim),
        knob(freq_i, 22.0),
    ]
    .spacing(1)
    .align_x(iced::Alignment::Center);

    row![
        column![gain_col, third_el]
            .spacing(2)
            .align_x(iced::Alignment::Center),
        column![text("").size(6), freq_col].align_x(iced::Alignment::Center),
    ]
    .spacing(9)
    .align_y(iced::Alignment::Start)
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

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::id::TrackId;

    #[test]
    fn automated_send_value_overrides_the_manual_mixer_value() {
        let track_id = TrackId::new();
        let bus_id = TrackId::new();
        let mut track = ProjectTrack::new(track_id, "Audio".to_string(), 0);
        track.sends.push((bus_id, 0.25));
        let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id });
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.1,
            curve: 0.0,
        });
        lane.insert_point(AutomationPoint {
            beat: 4.0,
            value: 0.9,
            curve: 0.0,
        });
        let automation = vec![lane];

        let displayed = effective_send_amount(&track, &automation, bus_id, 2.0);

        assert!((displayed - 0.5).abs() < 1e-6);
    }
}
