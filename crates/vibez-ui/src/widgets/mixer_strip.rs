//! A mix desk channel strip in one column: the V250 EQ, the sends, pan and
//! SEL/S/M, then the fader with its meter at the foot.

use iced::widget::{button, canvas, column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;

use crate::state::ProjectTrack;
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH};
use crate::widgets::console;
use crate::widgets::control_style;
use crate::widgets::effect_knob::EffectKnobWidget;
use crate::widgets::fader::FaderWidget;
use crate::widgets::knob::KnobWidget;
use crate::widgets::strip_eq::{self, EQ_H, EQ_W};
use crate::widgets::track_header::{view_editable_channel_name, ChannelNameStyle};

/// What kind of channel a mixer strip renders. Tracks get the full
/// set plus send knobs; buses trade sends/solo for a RETURN badge;
/// the master drops everything that makes no sense on a sum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StripRole {
    Track,
    Bus,
    Master,
}

pub const STRIP_W: f32 = EQ_W + PADDING.left + PADDING.right;
const CARD_RADIUS: f32 = 8.0;
const SELECTION_LINE_H: f32 = 2.0;
/// How far a selected strip's card leans toward the text colour.
const SELECTED_LIFT: f32 = 0.035;
const PADDING: iced::Padding = iced::Padding {
    top: 10.0,
    right: 8.0,
    bottom: 8.0,
    left: 8.0,
};
const HEADER_H: f32 = 20.0;
const GAP: f32 = 6.0;
const TITLE_H: f32 = 12.0;
const PAN_H: f32 = 24.0;
const BUTTONS_H: f32 = 17.0;
const DB_H: f32 = 14.0;
const METER_W: f32 = 10.0;
/// The fader's travel at the strip's natural height; a taller Mix card
/// gives it the extra, and a shorter one takes it back down to
/// [`SHORTEST_FADER_H`].
const NATURAL_FADER_H: f32 = 180.0;
const SHORTEST_FADER_H: f32 = 96.0;

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
    /// The strip's height, from [`strip_height`].
    pub height: f32,
    /// Which bank of buses the send rows show.
    pub send_bank: usize,
}

/// One row of send knobs.
const SEND_ROW_H: f32 = 22.0;
const SEND_KNOB: f32 = 20.0;
/// The sends section: its legend and a full bank of rows. Returns and the
/// master leave the same room so every strip is the same height.
const SENDS_H: f32 = TITLE_H + SENDS_PER_BANK as f32 * SEND_ROW_H;

/// A strip's natural height with `buses` send knobs on each track strip.
pub fn strip_height(buses: usize) -> f32 {
    let sends = if buses == 0 { 0.0 } else { SENDS_H + GAP };
    PADDING.top
        + HEADER_H
        + GAP
        + TITLE_H
        + GAP
        + EQ_H
        + GAP
        + sends
        + PAN_H
        + GAP
        + BUTTONS_H
        + GAP
        + NATURAL_FADER_H
        + DB_H
        + PADDING.bottom
}

/// The shortest a strip gets before the desk scrolls: everything whole, the
/// fader at its shortest.
pub fn shortest_strip_height(buses: usize) -> f32 {
    strip_height(buses) - (NATURAL_FADER_H - SHORTEST_FADER_H)
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
    let number = container(
        text(view.number.clone())
            .font(PERFORM_TECH)
            .size(9.5)
            .color(th::cap_ink()),
    )
    .padding([1, 5])
    .style(move |_: &Theme| container::Style {
        background: Some(track_color.into()),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let header = row![number, channel_name(track, is_master, &view), remove]
        .spacing(6)
        .height(HEADER_H)
        .align_y(iced::Alignment::Center);

    let mut strip = column![
        header,
        container(legend("EQ")).height(TITLE_H),
        strip_eq::view_strip_eq(track)
    ]
    .spacing(GAP);
    match role {
        StripRole::Track if !buses.is_empty() => {
            strip = strip.push(view_sends(
                track,
                buses,
                view.send_bank,
                view.playhead_beat,
                view.automation,
            ));
        }
        StripRole::Bus | StripRole::Master if !buses.is_empty() => {
            let label = if role == StripRole::Bus {
                "RETURN"
            } else {
                "OUTPUT"
            };
            strip = strip.push(container(legend(label)).height(SENDS_H));
        }
        StripRole::Track | StripRole::Bus | StripRole::Master => {}
    }
    let mut foot = column![].spacing(GAP);
    if is_master {
        // The sum has no pan or channel buttons; their room keeps its
        // strip level with the channels'.
        foot = foot
            .push(Space::with_height(PAN_H))
            .push(Space::with_height(BUTTONS_H));
    } else {
        let pan = KnobWidget::new(track.id, track.pan, track_color);
        foot = foot.push(
            container(
                row![
                    canvas(pan).width(PAN_H).height(PAN_H),
                    text(format!("PAN {}", format_pan(track.pan)))
                        .font(PERFORM_LABEL)
                        .size(8.5)
                        .color(th::text_dim()),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .center_x(Length::Fill)
            .height(PAN_H),
        );
        let mut buttons = row![].spacing(3).height(BUTTONS_H);
        if role == StripRole::Track {
            buttons = buttons.push(chip(
                "SEL",
                26.0,
                view.selected.then(th::accent),
                Message::Arrangement(
                    crate::domains::arrangement::ArrangementMsg::ToggleChannelSelection(track.id),
                ),
            ));
        }
        buttons = buttons
            .push(chip(
                "S",
                BUTTONS_H,
                track.solo.then(th::solo_active),
                Message::set_track_solo(track.id),
            ))
            .push(chip(
                "M",
                BUTTONS_H,
                track.mute.then(th::mute_active),
                Message::set_track_mute(track.id),
            ));
        foot = foot.push(buttons);
    }
    let fader = FaderWidget::new(track.id, track.gain, track_color);
    let meter = console::LedMeter {
        peak_l: view.peaks.0,
        peak_r: view.peaks.1,
    };
    let fader_block = column![
        row![
            canvas(fader).width(Length::Fill).height(Length::Fill),
            canvas(meter)
                .width(Length::Fixed(METER_W))
                .height(Length::Fill),
        ]
        .spacing(3)
        .height(Length::Fill),
        container(
            text(format_gain_db(track.gain))
                .font(PERFORM_TECH)
                .size(10)
                .color(th::text_dim()),
        )
        .center_x(Length::Fill)
        .height(DB_H),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    let strip = strip.push(foot).push(fader_block);

    let selected = view.selected;
    let body = container(
        column![
            selection_line(selected.then_some(track_color)),
            strip
                .padding(iced::Padding {
                    top: PADDING.top - SELECTION_LINE_H,
                    ..PADDING
                })
                .height(Length::Fill),
        ]
        .width(Length::Fixed(STRIP_W))
        .height(Length::Fill),
    )
    .height(Length::Fixed(view.height))
    .style(move |_: &Theme| container::Style {
        background: Some(
            if selected {
                th::blend(th::bg_elevated(), th::text(), SELECTED_LIFT)
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        border: iced::Border {
            radius: CARD_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Clicking anywhere on the strip chrome selects the track, so
    // the detail panel follows the mixer like it follows the
    // arrangement headers. Knobs, faders, and buttons still win
    // their own clicks.
    iced::widget::mouse_area(body).on_press(view.click).into()
}

/// The line across a selected strip's top, in its track colour; inset
/// clear of the card's rounded corners, and empty room on the others.
fn selection_line<'a>(colour: Option<Color>) -> Element<'a, Message> {
    container(
        container(Space::new(Length::Fill, SELECTION_LINE_H)).style(move |_: &Theme| {
            container::Style {
                background: colour.map(Into::into),
                ..Default::default()
            }
        }),
    )
    .padding([0.0, CARD_RADIUS])
    .into()
}

/// A section legend on the strip.
fn legend<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(label)
        .font(PERFORM_LABEL)
        .size(10)
        .color(th::text())
        .wrapping(text::Wrapping::None)
        .into()
}

/// A filled channel button `width` wide, lit in `lit` while it is on.
fn chip<'a>(
    label: &'a str,
    width: f32,
    lit: Option<Color>,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        container(text(label).font(PERFORM_LABEL).size(9))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(on_press)
    .width(width)
    .height(BUTTONS_H)
    .padding(0)
    .style(control_style::style(lit))
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

/// The channel's name in the strip's header, editable with a click like
/// the other channel names; the master's is fixed.
fn channel_name<'a>(
    track: &'a ProjectTrack,
    is_master: bool,
    view: &MixerStripView<'a>,
) -> Element<'a, Message> {
    let style = ChannelNameStyle {
        size: 12,
        font: PERFORM_LABEL,
        color: th::text(),
    };
    let name = if is_master {
        text(&track.name)
            .size(style.size)
            .font(style.font)
            .color(style.color)
            .into()
    } else {
        view_editable_channel_name(track, view.editing_name.then_some(view.edit_text), style)
    };
    container(name)
        .width(Length::Fill)
        .height(HEADER_H)
        .align_y(iced::alignment::Vertical::Center)
        .clip(true)
        .into()
}

/// Send rows on every strip, whatever the number of buses; more buses are
/// reached by flipping the bank, like aux banks on a desk.
pub const SENDS_PER_BANK: usize = 4;

/// How many banks `buses` buses fill; an empty desk still has one.
pub fn send_banks(buses: usize) -> usize {
    buses.div_ceil(SENDS_PER_BANK).max(1)
}

/// The buses on `bank`'s rows, top to bottom, with `None` for rows past the
/// last bus. A bank past the last shows the last.
pub fn bank_buses(buses: &[ProjectTrack], bank: usize) -> [Option<&ProjectTrack>; SENDS_PER_BANK] {
    let bank = bank.min(send_banks(buses.len()) - 1);
    std::array::from_fn(|row| buses.get(bank * SENDS_PER_BANK + row))
}

/// Send section on a track strip: a row per bus on the bank, with the bus's
/// colour, its name and its send knob.
fn view_sends<'a>(
    track: &'a ProjectTrack,
    buses: &'a [ProjectTrack],
    bank: usize,
    playhead_beat: f64,
    automation: &'a [vibez_core::automation::AutomationLane],
) -> Element<'a, Message> {
    let bank = bank.min(send_banks(buses.len()) - 1);
    let legend = if buses.len() > SENDS_PER_BANK {
        let first = bank * SENDS_PER_BANK + 1;
        format!("SENDS {first}–{}", first + SENDS_PER_BANK - 1)
    } else {
        "SENDS".to_string()
    };
    let dot = |colour: Color| {
        container(Space::new(6, 6)).style(move |_: &Theme| container::Style {
            background: Some(colour.into()),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    };
    let mut rows = column![container(self::legend(legend)).height(TITLE_H)];
    for bus in bank_buses(buses, bank) {
        let send: Element<'a, Message> = match bus {
            Some(bus) => {
                let colour = th::track_color(bus.color_index);
                let amount = effective_send_amount(track, automation, bus.id, playhead_beat);
                row![
                    dot(colour),
                    text(crate::typography::fit(
                        &bus.name,
                        EQ_W - SEND_KNOB - 22.0,
                        10.0,
                        iced::Font::DEFAULT
                    ))
                    .size(10)
                    .color(th::text_dim())
                    .wrapping(text::Wrapping::None)
                    .width(Length::Fill),
                    canvas(EffectKnobWidget::for_send(track.id, bus.id, amount, colour))
                        .width(SEND_KNOB)
                        .height(SEND_KNOB),
                ]
                .spacing(5)
                .height(SEND_ROW_H)
                .align_y(iced::Alignment::Center)
                .into()
            }
            None => row![
                dot(th::border()),
                text("-").size(10).color(th::text_muted())
            ]
            .spacing(5)
            .height(SEND_ROW_H)
            .align_y(iced::Alignment::Center)
            .into(),
        };
        rows = rows.push(send);
    }
    container(rows).height(SENDS_H).into()
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
    fn a_strip_is_as_wide_as_its_eq_with_the_fader_under_it() {
        assert_eq!(STRIP_W, EQ_W + PADDING.left + PADDING.right);
    }

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
