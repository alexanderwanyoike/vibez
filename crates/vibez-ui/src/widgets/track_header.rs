use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, row, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::piano_roll::PianoRollMsg;
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
pub fn view_track_header<'a>(
    track: &'a UiTrack,
    selected: bool,
    editing_name: bool,
    edit_text: &'a str,
) -> Element<'a, Message> {
    let track_color = th::track_color(track.color_index);

    // Row 1: Track type icon + name + "+" add clip button + delete button
    let type_icon = match track.kind {
        TrackKind::Audio => icons::icon(icons::AUDIO_WAVEFORM)
            .size(12)
            .color(track_color),
        TrackKind::Instrument(_) | TrackKind::Midi => {
            icons::icon(icons::MUSIC).size(12).color(track_color)
        }
    };

    // Name: if editing, show text_input; if selected, click starts rename; else click selects
    let name_widget: Element<'_, Message> = if editing_name {
        text_input("Name", edit_text)
            .on_input(Message::EditNameText)
            .on_submit(Message::FinishEditing)
            .size(13)
            .width(Length::Fill)
            .into()
    } else {
        let name_color = if track.mute { th::TEXT_DIM } else { th::TEXT };
        let msg = if selected {
            Message::StartEditingTrackName(track.id)
        } else {
            Message::select_track(track.id)
        };
        button(text(&track.name).size(13).color(name_color))
            .on_press(msg)
            .padding(0)
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: th::TEXT,
                border: iced::Border::default(),
                ..Default::default()
            })
            .into()
    };

    let add_btn = match track.kind {
        TrackKind::Audio => button(icons::icon(icons::PLUS).size(11).color(th::TEXT_DIM))
            .on_press(Message::AddClipToTrack(track.id))
            .padding([2, 6]),
        TrackKind::Instrument(_) | TrackKind::Midi => {
            button(icons::icon(icons::PLUS).size(11).color(th::TEXT_DIM))
                .on_press(Message::PianoRoll(PianoRollMsg::AddNoteClipToTrack(
                    track.id,
                )))
                .padding([2, 6])
        }
    };

    let delete_btn = button(icons::icon(icons::TRASH_2).size(10).color(th::TEXT_DIM))
        .on_press(Message::remove_track(track.id))
        .padding([2, 4])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT_DIM,
                border: iced::Border::default(),
                ..Default::default()
            }
        });

    let name_row = row![
        type_icon,
        name_widget,
        horizontal_space(),
        add_btn,
        delete_btn
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center);

    // Row 2: Mute/Solo buttons with filled backgrounds when active
    let mute_btn = {
        let label = text("M").size(11);
        if track.mute {
            button(label.color(th::BG_DARK))
                .on_press(Message::set_track_mute(track.id))
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
                .on_press(Message::set_track_mute(track.id))
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
                .on_press(Message::set_track_solo(track.id))
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
                .on_press(Message::set_track_solo(track.id))
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

    let card = container(header_with_bar).style(move |_theme: &Theme| container::Style {
        background: Some(
            if selected {
                th::TRACK_BG_SELECTED
            } else {
                th::BG_SURFACE
            }
            .into(),
        ),
        border: iced::Border {
            color: if selected { th::ACCENT_DIM } else { th::BORDER },
            width: if selected { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    // The whole header column selects the track. Child widgets
    // (name, M/S, add, delete, fader) capture their own clicks first,
    // so this only fires on otherwise-dead header space.
    mouse_area(card)
        .on_press(Message::select_track(track.id))
        .into()
}
