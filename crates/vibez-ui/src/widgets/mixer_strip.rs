//! A mix desk channel strip laid out like an SSL console: channel number,
//! four-band EQ, sends, pan, SEL/SOLO/MUTE, a long fader with an LED meter
//! and a dark name display, in the theme's colours.

use iced::widget::{button, canvas, column, container, horizontal_space, row, text, Space};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use vibez_core::effect::EffectType;

use crate::state::{ProjectTrack, UiEffect};
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH};
use crate::widgets::console;
use crate::widgets::effect_knob::EffectKnobWidget;
use crate::widgets::fader::FaderWidget;
use crate::widgets::knob::KnobWidget;

/// What kind of channel a mixer strip renders. Tracks get the full
/// set plus send knobs; buses trade sends/solo for a RETURN badge;
/// the master drops everything that makes no sense on a sum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StripRole {
    Track,
    Bus,
    Master,
}

pub const STRIP_W: f32 = 116.0;
const PAN_H: f32 = 56.0;
const BUTTONS_H: f32 = 22.0;

/// Per-render inputs that are not part of the project-owned channel itself.
pub struct MixerStripView<'a> {
    pub peaks: (f32, f32),
    pub selected: bool,
    /// Sent when the strip's chrome is clicked.
    pub click: Message,
    /// The strip's number on the desk: 01 for the first track, R1 for the
    /// first return.
    pub number: String,
    pub editing_name: bool,
    pub edit_text: &'a str,
    pub playhead_beat: f64,
    pub automation: &'a [vibez_core::automation::AutomationLane],
    /// The strip's height; the fader takes what the other sections leave.
    pub height: f32,
}

/// Everything on a strip but its fader, measured on the real desk.
const SECTIONS_H: f32 = 560.0;
/// One row of send knobs, and the SENDS legend above them.
const SEND_ROW_H: f32 = 30.0;
const SENDS_LEGEND_H: f32 = 14.0;
/// The shortest fader worth having; below it the desk scrolls instead.
const MIN_FADER_H: f32 = 220.0;

/// The shortest a strip can be while keeping a usable fader, with `buses`
/// send knobs on each track strip.
pub fn min_strip_height(buses: usize) -> f32 {
    let sends = if buses == 0 {
        0.0
    } else {
        SENDS_LEGEND_H + buses.div_ceil(3) as f32 * SEND_ROW_H
    };
    SECTIONS_H + sends + MIN_FADER_H
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

    let remove: Element<'a, Message> = match role {
        StripRole::Track => icon_button(icons::TRASH_2, Message::remove_track(track.id)),
        StripRole::Bus => icon_button(icons::X, Message::remove_bus(track.id)),
        StripRole::Master => Space::with_width(0).into(),
    };
    let header = row![
        container(
            text(view.number.clone())
                .font(PERFORM_TECH)
                .size(10)
                .color(Color::from_rgb(1.0, 0.604, 0.239)),
        )
        .padding([1, 5])
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgb(0.102, 0.059, 0.031).into()),
            border: iced::Border {
                color: Color::BLACK,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        }),
        horizontal_space(),
        container(Space::new(20, 5)).style(move |_: &Theme| container::Style {
            background: Some(track_color.into()),
            border: iced::Border {
                radius: 1.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
        remove,
    ]
    .spacing(5)
    .height(26)
    .align_y(iced::Alignment::Center);

    let mut strip = column![header, rule(), view_strip_eq(track), rule()];
    match role {
        StripRole::Track if !buses.is_empty() => {
            strip = strip.push(view_sends(
                track,
                buses,
                view.playhead_beat,
                view.automation,
            ));
            strip = strip.push(rule());
        }
        StripRole::Track => {}
        StripRole::Bus | StripRole::Master => {}
    }
    if is_master {
        // The sum has no pan or channel buttons; their room keeps its fader
        // level with the channels'.
        strip = strip
            .push(Space::with_height(PAN_H))
            .push(rule())
            .push(Space::with_height(BUTTONS_H))
            .push(rule());
    } else {
        let pan = KnobWidget::new(track.id, track.pan, track_color);
        strip = strip
            .push(
                column![
                    canvas(pan)
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0)),
                    text(format!("PAN {}", format_pan(track.pan)))
                        .font(PERFORM_TECH)
                        .size(9)
                        .color(th::text_dim()),
                ]
                .spacing(1)
                .width(Length::Fill)
                .height(PAN_H)
                .align_x(iced::Alignment::Center),
            )
            .push(rule());
        let mut buttons = row![].spacing(4);
        if role == StripRole::Track {
            buttons = buttons.push(led_button(
                "SEL",
                view.selected.then_some(th::accent()),
                Message::Arrangement(
                    crate::domains::arrangement::ArrangementMsg::ToggleChannelSelection(track.id),
                ),
            ));
        }
        buttons = buttons
            .push(led_button(
                "S",
                track.solo.then_some(console::LED_YELLOW),
                Message::set_track_solo(track.id),
            ))
            .push(led_button(
                "M",
                track.mute.then_some(console::LED_RED),
                Message::set_track_mute(track.id),
            ));
        strip = strip
            .push(
                container(buttons)
                    .width(Length::Fill)
                    .height(BUTTONS_H)
                    .center_x(Length::Fill)
                    .center_y(BUTTONS_H),
            )
            .push(rule());
    }

    let fader = FaderWidget::new(track.id, track.gain, track_color);
    let meter = console::LedMeter {
        peak_l: view.peaks.0,
        peak_r: view.peaks.1,
    };
    strip = strip
        .push(
            container(
                row![
                    canvas(fader)
                        .width(Length::Fixed(50.0))
                        .height(Length::Fill),
                    canvas(meter)
                        .width(Length::Fixed(14.0))
                        .height(Length::Fill),
                ]
                .spacing(4)
                .height(Length::Fill),
            )
            .center_x(Length::Fill)
            .height(Length::Fill),
        )
        .push(
            container(
                text(format_gain_db(track.gain))
                    .font(PERFORM_TECH)
                    .size(10)
                    .color(th::text()),
            )
            .center_x(Length::Fill),
        )
        .push(name_display(track, is_master, track_color, &view));

    let selected = view.selected;
    let body = container(
        strip
            .spacing(5)
            .padding([4, 6])
            .width(Length::Fixed(STRIP_W))
            .height(Length::Fill),
    )
    .height(Length::Fixed(view.height))
    .style(move |_: &Theme| container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: if selected { th::accent() } else { th::border() },
            width: if selected { 2.0 } else { 1.0 },
            radius: 2.0.into(),
        },
        ..Default::default()
    });

    // Clicking anywhere on the strip chrome selects the track, so
    // the detail panel follows the mixer like it follows the
    // arrangement headers. Knobs, faders, and buttons still win
    // their own clicks.
    iced::widget::mouse_area(body).on_press(view.click).into()
}

/// The silkscreen rule between a strip's sections.
fn rule<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fill, 1))
        .style(|_: &Theme| container::Style {
            background: Some(th::border().into()),
            ..Default::default()
        })
        .into()
}

/// A section legend in the strip's silkscreen style.
fn legend<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .font(PERFORM_LABEL)
        .size(9)
        .color(th::text())
        .into()
}

/// A square console button with an LED that lights in `lit`.
fn led_button<'a>(label: &'a str, lit: Option<Color>, on_press: Message) -> Element<'a, Message> {
    let led = container(Space::new(6, 6)).style(move |_: &Theme| container::Style {
        background: Some(lit.unwrap_or_else(th::bg_dark).into()),
        border: iced::Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
            width: 0.5,
            radius: 3.0.into(),
        },
        ..Default::default()
    });
    button(
        row![
            led,
            text(label)
                .font(PERFORM_LABEL)
                .size(8)
                .color(th::text())
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_press)
    .padding([3, 4])
    .style(|_: &Theme, status| button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn icon_button<'a>(icon: char, on_press: Message) -> Element<'a, Message> {
    button(icons::icon(icon).size(9))
        .on_press(on_press)
        .padding([1, 3])
        .style(|_: &Theme, status| button::Style {
            background: None,
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::danger()
            } else {
                th::text_dim()
            },
            border: iced::Border::default(),
            ..Default::default()
        })
        .into()
}

/// The channel's name on a dark display at the foot of the strip, editable
/// with a click like the other channel names.
fn name_display<'a>(
    track: &'a ProjectTrack,
    is_master: bool,
    colour: Color,
    view: &MixerStripView<'a>,
) -> Element<'a, Message> {
    let name: Element<'a, Message> = if view.editing_name {
        iced::widget::text_input("Name", view.edit_text)
            .on_input(|value| Message::View(crate::domains::view::ViewMsg::EditNameText(value)))
            .on_submit(Message::View(crate::domains::view::ViewMsg::FinishEditing))
            .size(11)
            .width(Length::Fill)
            .into()
    } else {
        let label = text(crate::widgets::clip_slot::fit(
            &track.name,
            STRIP_W - 34.0,
            11.0,
        ))
        .font(PERFORM_LABEL)
        .size(11)
        .color(console::DISPLAY_TEXT);
        if is_master {
            label.into()
        } else {
            button(label)
                .on_press(Message::View(
                    crate::domains::view::ViewMsg::StartEditingTrackName {
                        track_id: track.id,
                        name: track.name.clone(),
                    },
                ))
                .padding(0)
                .style(|_: &Theme, _| button::Style {
                    background: None,
                    text_color: console::DISPLAY_TEXT,
                    border: iced::Border::default(),
                    ..Default::default()
                })
                .into()
        }
    };
    container(
        row![
            container(Space::new(3, 12)).style(move |_: &Theme| container::Style {
                background: Some(colour.into()),
                ..Default::default()
            }),
            name
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 5])
    .width(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(console::DISPLAY.into()),
        border: iced::Border {
            color: Color::BLACK,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Send rows on every strip, whatever the number of buses; more buses are
/// reached by flipping the bank, like aux banks on a desk.
pub const SENDS_PER_BANK: usize = 4;

/// How many banks `buses` buses fill; an empty desk still has one.
pub fn send_banks(buses: usize) -> usize {
    1
}

/// The buses on `bank`'s rows, top to bottom, with `None` for rows past the
/// last bus. A bank past the last shows the last.
pub fn bank_buses(buses: &[ProjectTrack], bank: usize) -> [Option<&ProjectTrack>; SENDS_PER_BANK] {
    [None; SENDS_PER_BANK]
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
        return container(led_button(
            "+ EQ",
            None,
            Message::add_effect(track.id, EffectType::Eq),
        ))
        .center_x(Length::Fill)
        .into();
    };

    let header = row![
        legend("EQ"),
        horizontal_space(),
        led_button(
            "IN",
            (!eq.bypass).then_some(console::LED_GREEN),
            Message::toggle_effect_bypass(track.id, eq.id),
        ),
    ]
    .align_y(iced::Alignment::Center);

    // (gain idx, freq idx, q/bell idx, bell?, color, label)
    let rows: [(usize, usize, usize, bool, Color, &str); 4] = [
        (9, 10, 11, true, th::eq_hf(), "HF"),
        (6, 7, 8, false, th::eq_hmf(), "HMF"),
        (3, 4, 5, false, th::eq_lmf(), "LMF"),
        (0, 1, 2, true, th::eq_lf(), "LF"),
    ];
    let mut bands = column![header].spacing(6);
    for (gain_i, freq_i, third_i, bell_toggle, color, label) in rows {
        bands = bands.push(view_eq_band(
            track.id,
            eq,
            gain_i,
            freq_i,
            third_i,
            bell_toggle,
            color,
            label,
        ));
    }
    bands.into()
}

/// Console stagger: the gain knob under the band's legend on the left,
/// frequency over Q (or the bell switch on the shelving bands) on the right.
#[allow(clippy::too_many_arguments)]
fn view_eq_band<'a>(
    track_id: vibez_core::id::TrackId,
    eq: &'a UiEffect,
    gain_i: usize,
    freq_i: usize,
    third_i: usize,
    bell_toggle: bool,
    color: Color,
    label: &'static str,
) -> Element<'a, Message> {
    let knob = |i: usize, size: f32, caption: &'static str| -> Element<'a, Message> {
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
        column![
            canvas(w)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size)),
            text(caption)
                .font(PERFORM_LABEL)
                .size(8)
                .color(th::text_dim()),
        ]
        .align_x(iced::Alignment::Center)
        .into()
    };

    let third: Element<'a, Message> = if bell_toggle {
        let bell_on = eq.params.get(third_i).copied().unwrap_or(0.0) >= 0.5;
        led_button(
            "BELL",
            bell_on.then_some(color),
            Message::set_effect_param(track_id, eq.id, third_i, if bell_on { 0.0 } else { 1.0 }),
        )
    } else {
        knob(third_i, 28.0, "Q")
    };

    row![
        column![
            text(label).font(PERFORM_LABEL).size(10).color(color),
            knob(gain_i, 34.0, "GAIN"),
        ]
        .spacing(2)
        .align_x(iced::Alignment::Center),
        column![
            knob(
                freq_i,
                28.0,
                if freq_i == 10 || freq_i == 7 {
                    "kHz"
                } else {
                    "Hz"
                }
            ),
            third,
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center),
    ]
    .spacing(14)
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

    #[test]
    fn a_send_bank_shows_four_buses_and_clamps_past_the_last() {
        let buses: Vec<_> = (0..6)
            .map(|index| ProjectTrack::new(TrackId::new(), format!("Bus {index}"), 0))
            .collect();
        let names = |bank| bank_buses(&buses, bank).map(|bus| bus.map(|bus| bus.name.clone()));
        let named = |name: &str| Some(name.to_string());
        assert_eq!(
            names(0),
            [
                named("Bus 0"),
                named("Bus 1"),
                named("Bus 2"),
                named("Bus 3")
            ]
        );
        assert_eq!(names(1), [named("Bus 4"), named("Bus 5"), None, None]);
        assert_eq!(names(7), names(1), "a bank past the last shows the last");
        assert_eq!(send_banks(0), 1);
        assert_eq!(send_banks(4), 1);
        assert_eq!(send_banks(5), 2);
    }
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
