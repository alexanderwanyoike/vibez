use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;
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
        TrackKind::Instrument(_) => icons::icon(icons::MUSIC).size(10).color(track_color),
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
                .on_press(Message::SetTrackMute(track.id))
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
                .on_press(Message::SetTrackMute(track.id))
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
                .on_press(Message::SetTrackSolo(track.id))
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
                .on_press(Message::SetTrackSolo(track.id))
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

    let strip = column![
        name_row,
        knob_canvas,
        pan_label,
        fader_meter,
        gain_label,
        mute_solo_row,
    ]
    .spacing(4)
    .padding(8)
    .width(Length::Fixed(90.0))
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
