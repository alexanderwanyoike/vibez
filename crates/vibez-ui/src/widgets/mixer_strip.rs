use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as vibez_theme;
use crate::widgets::fader::FaderWidget;
use crate::widgets::knob::KnobWidget;
use crate::widgets::vu_meter::VuMeterWidget;

/// Render a single mixer channel strip for a track.
pub fn view_mixer_strip(track: &UiTrack) -> Element<Message> {
    // Track name
    let name = text(&track.name)
        .size(11)
        .color(vibez_theme::TEXT)
        .width(Length::Fill);

    // Pan knob
    let knob = KnobWidget::new(track.id, track.pan);
    let knob_canvas: Element<Message> = canvas(knob)
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .into();

    let pan_label = text(format_pan(track.pan))
        .size(9)
        .color(vibez_theme::TEXT_DIM);

    // Fader
    let fader = FaderWidget::new(track.id, track.gain);
    let fader_canvas: Element<Message> = canvas(fader)
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(100.0))
        .into();

    let gain_label = text(format_gain_db(track.gain))
        .size(9)
        .color(vibez_theme::TEXT_DIM);

    // VU meter for this track
    let meter = VuMeterWidget {
        peak_l: track.peak_l,
        peak_r: track.peak_r,
    };
    let meter_canvas: Element<Message> = canvas(meter)
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(100.0))
        .into();

    // Mute button
    let mute_btn = if track.mute {
        button(text("M").size(11).color(vibez_theme::MUTE_ACTIVE))
            .on_press(Message::SetTrackMute(track.id))
            .padding([4, 8])
    } else {
        button(text("M").size(11).color(vibez_theme::TEXT_DIM))
            .on_press(Message::SetTrackMute(track.id))
            .padding([4, 8])
    };

    // Solo button
    let solo_btn = if track.solo {
        button(text("S").size(11).color(vibez_theme::SOLO_ACTIVE))
            .on_press(Message::SetTrackSolo(track.id))
            .padding([4, 8])
    } else {
        button(text("S").size(11).color(vibez_theme::TEXT_DIM))
            .on_press(Message::SetTrackSolo(track.id))
            .padding([4, 8])
    };

    let mute_solo_row = row![mute_btn, solo_btn].spacing(2);

    // Add clip button
    let add_clip_btn = button(text("+").size(11))
        .on_press(Message::AddClipToTrack(track.id))
        .padding([2, 6]);

    // Fader + meter side by side
    let fader_meter = row![fader_canvas, meter_canvas].spacing(2);

    let strip = column![
        name,
        knob_canvas,
        pan_label,
        fader_meter,
        gain_label,
        mute_solo_row,
        add_clip_btn,
    ]
    .spacing(4)
    .padding(6)
    .width(Length::Fixed(72.0))
    .align_x(iced::Alignment::Center);

    container(strip)
        .style(|_theme: &Theme| container::Style {
            background: Some(vibez_theme::BG_SURFACE.into()),
            border: iced::Border {
                color: vibez_theme::TEXT_DIM,
                width: 0.5,
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
