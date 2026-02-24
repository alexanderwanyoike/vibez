use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;
use crate::widgets::fader::HorizontalFaderWidget;
use crate::widgets::vu_meter::HorizontalVuMeterWidget;
use vibez_core::midi::TrackKind;

/// Width of the track header panel in the arrangement view.
pub const TRACK_HEADER_WIDTH: f32 = 220.0;

/// Total width including the 3px color bar on the left edge.
pub const TRACK_HEADER_TOTAL_WIDTH: f32 = TRACK_HEADER_WIDTH + 3.0;

/// Render the track header for the arrangement view.
pub fn view_track_header(track: &UiTrack) -> Element<'_, Message> {
    let track_color = th::track_color(track.color_index);

    // Row 1: Track type icon + name + "+" add clip button
    let type_icon = match track.kind {
        TrackKind::Audio => icons::icon(icons::AUDIO_WAVEFORM)
            .size(12)
            .color(track_color),
        TrackKind::Instrument(_) => icons::icon(icons::MUSIC).size(12).color(track_color),
    };

    let name = text(&track.name)
        .size(13)
        .color(if track.mute { th::TEXT_DIM } else { th::TEXT });

    let add_btn = match track.kind {
        TrackKind::Audio => button(icons::icon(icons::PLUS).size(11).color(th::TEXT_DIM))
            .on_press(Message::AddClipToTrack(track.id))
            .padding([2, 6]),
        TrackKind::Instrument(_) => button(icons::icon(icons::PLUS).size(11).color(th::TEXT_DIM))
            .on_press(Message::AddNoteClipToTrack(track.id))
            .padding([2, 6]),
    };

    let name_row = row![type_icon, name, add_btn]
        .spacing(6)
        .align_y(iced::Alignment::Center);

    // Row 2: Mute/Solo buttons with filled backgrounds when active
    let mute_btn = {
        let label = text("M").size(11);
        if track.mute {
            button(label.color(th::BG_DARK))
                .on_press(Message::SetTrackMute(track.id))
                .padding([3, 8])
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
                .padding([3, 8])
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

    let solo_btn = {
        let label = text("S").size(11);
        if track.solo {
            button(label.color(th::BG_DARK))
                .on_press(Message::SetTrackSolo(track.id))
                .padding([3, 8])
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
                .padding([3, 8])
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

    // Row 3: Horizontal gain fader (spans width)
    let fader = HorizontalFaderWidget::new(track.id, track.gain, track_color);
    let fader_canvas: Element<'_, Message> = canvas(fader)
        .width(Length::Fill)
        .height(Length::Fixed(18.0))
        .into();

    // Row 4: Horizontal VU meter (spans width)
    let meter = HorizontalVuMeterWidget::new(track.peak_l, track.peak_r, track_color);
    let meter_canvas: Element<'_, Message> = canvas(meter)
        .width(Length::Fill)
        .height(Length::Fixed(6.0))
        .into();

    let header = column![name_row, mute_solo_row, fader_canvas, meter_canvas]
        .spacing(4)
        .padding([6, 6])
        .width(Length::Fixed(TRACK_HEADER_WIDTH));

    // Color bar on the left edge
    let color_bar = container(text(""))
        .width(Length::Fixed(3.0))
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(track_color.into()),
            ..Default::default()
        });

    let header_with_bar = row![color_bar, header].height(Length::Fill);

    container(header_with_bar)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}
