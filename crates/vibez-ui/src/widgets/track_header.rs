use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as vibez_theme;
use crate::widgets::fader::HorizontalFaderWidget;
use crate::widgets::vu_meter::HorizontalVuMeterWidget;

/// Width of the track header panel in the arrangement view.
pub const TRACK_HEADER_WIDTH: f32 = 180.0;

/// Render the track header for the arrangement view.
/// Compact layout: name + "+" | [M] [S] | horizontal gain fader + VU meter.
pub fn view_track_header(track: &UiTrack) -> Element<Message> {
    // Row 1: Track name + "+" add clip button
    let name = text(&track.name).size(12).color(if track.mute {
        vibez_theme::TEXT_DIM
    } else {
        vibez_theme::TEXT
    });

    let add_clip_btn = button(text("+").size(11))
        .on_press(Message::AddClipToTrack(track.id))
        .padding([2, 6]);

    let name_row = row![name, add_clip_btn]
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Row 2: Mute/Solo buttons
    let mute_btn = if track.mute {
        button(text("M").size(10).color(vibez_theme::MUTE_ACTIVE))
            .on_press(Message::SetTrackMute(track.id))
            .padding([2, 6])
    } else {
        button(text("M").size(10).color(vibez_theme::TEXT_DIM))
            .on_press(Message::SetTrackMute(track.id))
            .padding([2, 6])
    };

    let solo_btn = if track.solo {
        button(text("S").size(10).color(vibez_theme::SOLO_ACTIVE))
            .on_press(Message::SetTrackSolo(track.id))
            .padding([2, 6])
    } else {
        button(text("S").size(10).color(vibez_theme::TEXT_DIM))
            .on_press(Message::SetTrackSolo(track.id))
            .padding([2, 6])
    };

    let mute_solo_row = row![mute_btn, solo_btn].spacing(2);

    // Row 3: Horizontal gain fader (spans width)
    let fader = HorizontalFaderWidget::new(track.id, track.gain);
    let fader_canvas: Element<Message> = canvas(fader)
        .width(Length::Fill)
        .height(Length::Fixed(14.0))
        .into();

    // Row 4: Horizontal VU meter (spans width)
    let meter = HorizontalVuMeterWidget {
        peak_l: track.peak_l,
        peak_r: track.peak_r,
    };
    let meter_canvas: Element<Message> = canvas(meter)
        .width(Length::Fill)
        .height(Length::Fixed(8.0))
        .into();

    let header = column![name_row, mute_solo_row, fader_canvas, meter_canvas]
        .spacing(2)
        .padding(6)
        .width(Length::Fixed(TRACK_HEADER_WIDTH));

    container(header)
        .style(|_theme: &Theme| container::Style {
            background: Some(vibez_theme::BG_SURFACE.into()),
            ..Default::default()
        })
        .into()
}
