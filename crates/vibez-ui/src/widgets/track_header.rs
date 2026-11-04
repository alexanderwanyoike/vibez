use std::borrow::Borrow;

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, pick_list, row, text,
    text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::automation::AutomationMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use crate::icons;
use crate::message::Message;
use crate::state::ProjectTrack;
use crate::theme as th;
use crate::widgets::fader::HorizontalFaderWidget;
use crate::widgets::vu_meter::HorizontalVuMeterWidget;
use vibez_core::midi::TrackKind;
use vibez_core::track::{AudioInputRoute, InputMonitoring};

/// Width of the track header panel in the arrangement view.
pub const TRACK_HEADER_WIDTH: f32 = 220.0;

/// Total width including the 3px color bar on the left edge.
pub const TRACK_HEADER_TOTAL_WIDTH: f32 = TRACK_HEADER_WIDTH + 3.0;

/// How an Arrange track header is shown.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackHeaderFlags {
    pub selected: bool,
    pub editing_name: bool,
    pub automation_open: bool,
    pub hovered: bool,
}

impl TrackHeaderFlags {
    /// An audio track's input controls show while its header is hovered,
    /// and stay while the track is armed, resampling or monitoring.
    fn shows_audio_input(self, track: &ProjectTrack, armed: bool) -> bool {
        track.kind == TrackKind::Audio
            && (self.hovered
                || armed
                || track.audio_input_route.resample_source().is_some()
                || track.input_monitoring != InputMonitoring::Off)
    }

    /// Whether the quiet add action is shown and clickable.
    fn shows_actions(self) -> bool {
        self.hovered
    }

    /// Whether the automation toggle is shown and clickable.
    fn shows_automation_toggle(self) -> bool {
        self.hovered || self.automation_open
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TrackHeaderRecordingView<'a> {
    pub input_channels: u16,
    pub armed: bool,
    pub input_peaks: Option<(f32, f32)>,
    pub output_peaks: (f32, f32),
    pub source_tracks: &'a [ProjectTrack],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioInputChoice {
    route: AudioInputRoute,
    label: String,
}

impl std::fmt::Display for AudioInputChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn compact_input_pick_list<'a, T>(
    options: impl Borrow<[T]> + 'a,
    selected: T,
    on_selected: impl Fn(T) -> Message + 'a,
    width: f32,
    highlighted: bool,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, Some(selected), on_selected)
        .width(Length::Fixed(width))
        .padding([3, 5])
        .text_size(9)
        .style(move |_theme: &Theme, status| {
            let engaged = matches!(
                status,
                pick_list::Status::Hovered | pick_list::Status::Opened
            );
            pick_list::Style {
                text_color: if highlighted {
                    th::accent()
                } else {
                    th::text()
                },
                placeholder_color: th::text_muted(),
                handle_color: if engaged || highlighted {
                    th::accent()
                } else {
                    th::text_dim()
                },
                background: th::bg_dark().into(),
                border: iced::Border {
                    color: if engaged || highlighted {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

/// Inline-editable channel name shared by arrangement headers and
/// mixer strips. The caller decides which channel roles expose it.
pub fn view_editable_channel_name<'a>(
    track: &'a ProjectTrack,
    editing_name: bool,
    edit_text: &'a str,
    size: u16,
    color: iced::Color,
) -> Element<'a, Message> {
    if editing_name {
        text_input("Name", edit_text)
            .on_input(|value| Message::View(ViewMsg::EditNameText(value)))
            .on_submit(Message::View(ViewMsg::FinishEditing))
            .size(size)
            .width(Length::Fill)
            .into()
    } else {
        button(text(&track.name).size(size).color(color))
            .on_press(Message::View(ViewMsg::StartEditingTrackName {
                track_id: track.id,
                name: track.name.clone(),
            }))
            .padding(0)
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: th::text(),
                border: iced::Border::default(),
                ..Default::default()
            })
            .into()
    }
}

/// Render the track header for the arrangement view.
pub fn view_track_header<'a>(
    track: &'a ProjectTrack,
    flags: TrackHeaderFlags,
    edit_text: &'a str,
    recording: TrackHeaderRecordingView<'a>,
) -> Element<'a, Message> {
    let TrackHeaderFlags {
        selected,
        editing_name,
        automation_open,
        ..
    } = flags;
    let TrackHeaderRecordingView {
        input_channels: audio_input_channels,
        armed,
        input_peaks,
        output_peaks,
        source_tracks,
    } = recording;
    let track_color = th::track_color(track.color_index);

    // Row 1: Track type icon + name + "+" add clip button
    let type_icon = match track.kind {
        TrackKind::Audio => icons::icon(icons::AUDIO_WAVEFORM)
            .size(12)
            .color(track_color),
        TrackKind::Instrument(_) | TrackKind::Midi => {
            icons::icon(icons::MUSIC).size(12).color(track_color)
        }
    };

    let name_color = if track.mute {
        th::text_dim()
    } else {
        th::text()
    };
    let name_widget = view_editable_channel_name(track, editing_name, edit_text, 13, name_color);

    // Hidden actions stay laid out, so nothing shifts when the pointer
    // arrives; they can only be clicked while the header is hovered.
    let action_color = if flags.shows_actions() {
        th::text_dim()
    } else {
        iced::Color::TRANSPARENT
    };

    let add_message = match track.kind {
        TrackKind::Audio => Message::AddClipToTrack(track.id),
        TrackKind::Instrument(_) | TrackKind::Midi => {
            Message::PianoRoll(PianoRollMsg::AddNoteClipToTrack(track.id))
        }
    };
    let add_btn = button(icons::icon(icons::PLUS).size(11).color(action_color))
        .on_press_maybe(flags.shows_actions().then_some(add_message))
        .padding([2, 4])
        .style(quiet_action_style);

    let auto_btn = {
        let color = if automation_open {
            th::accent()
        } else {
            action_color
        };
        button(icons::icon(icons::SLIDERS_VERTICAL).size(10).color(color))
            .on_press_maybe(
                flags
                    .shows_automation_toggle()
                    .then_some(Message::Automation(AutomationMsg::ToggleTrackLanes(
                        track.id,
                    ))),
            )
            .padding([2, 4])
            .style(quiet_action_style)
    };

    let name_row = row![
        type_icon,
        name_widget,
        horizontal_space(),
        auto_btn,
        add_btn
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center);

    // Row 2: Mute/Solo buttons with filled backgrounds when active
    let mute_btn = {
        let label = text("M").size(11);
        if track.mute {
            button(label.color(th::bg_dark()))
                .on_press(Message::set_track_mute(track.id))
                .padding([3, 8])
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
                .padding([3, 8])
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

    let solo_btn = {
        let label = text("S").size(11);
        if track.solo {
            button(label.color(th::bg_dark()))
                .on_press(Message::set_track_solo(track.id))
                .padding([3, 8])
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
                .padding([3, 8])
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

    let track_controls: Element<'_, Message> = if flags.shows_audio_input(track, armed) {
        let arm_color = if armed { th::danger() } else { th::text_dim() };
        let arm_icon = if armed {
            icons::CIRCLE_DOT
        } else {
            icons::CIRCLE
        };
        let arm_btn = button(icons::icon(arm_icon).size(11).color(arm_color))
            .on_press(Message::ToggleAudioTrackArm(track.id))
            .width(Length::Fixed(25.0))
            .padding([4, 6])
            .style(move |_theme: &Theme, status| {
                let engaged = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(
                        if armed {
                            th::blend(th::danger(), th::bg_dark(), 0.28)
                        } else if engaged {
                            th::bg_hover()
                        } else {
                            th::bg_dark()
                        }
                        .into(),
                    ),
                    text_color: arm_color,
                    border: iced::Border {
                        color: if armed { th::danger() } else { th::border() },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            });
        let mut routes = Vec::new();
        for channel in 0..audio_input_channels {
            let route = AudioInputRoute::Mono { channel };
            routes.push(AudioInputChoice {
                route,
                label: route.to_string(),
            });
        }
        for left in (0..audio_input_channels.saturating_sub(1)).step_by(2) {
            let route = AudioInputRoute::Stereo { left };
            routes.push(AudioInputChoice {
                route,
                label: route.to_string(),
            });
        }
        routes.extend(
            source_tracks
                .iter()
                .filter(|source| source.id != track.id && source.is_playable_midi_target())
                .map(|source| AudioInputChoice {
                    route: AudioInputRoute::Resample {
                        track_id: source.id,
                    },
                    label: format!("RS · {}", source.name),
                }),
        );
        let selected_route = routes
            .iter()
            .find(|choice| choice.route == track.audio_input_route)
            .cloned()
            .unwrap_or_else(|| AudioInputChoice {
                route: track.audio_input_route,
                label: match track.audio_input_route {
                    AudioInputRoute::Resample { .. } => "RS · Missing Track".into(),
                    route => route.to_string(),
                },
            });
        if routes.is_empty() {
            routes.push(selected_route.clone());
        }
        let is_resample = track.audio_input_route.resample_source().is_some();
        let track_id = track.id;
        let route = compact_input_pick_list(
            routes,
            selected_route,
            move |choice| Message::SetAudioTrackInputRoute(track_id, choice.route),
            if is_resample { 119.0 } else { 78.0 },
            is_resample,
        );
        if is_resample {
            row![mute_btn, solo_btn, arm_btn, route]
                .spacing(3)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            let track_id = track.id;
            let monitor = compact_input_pick_list(
                InputMonitoring::ALL,
                track.input_monitoring,
                move |value| Message::SetAudioTrackMonitoring(track_id, value),
                42.0,
                track.input_monitoring != InputMonitoring::Off,
            );
            row![mute_btn, solo_btn, arm_btn, route, monitor]
                .spacing(3)
                .align_y(iced::Alignment::Center)
                .into()
        }
    } else {
        row![mute_btn, solo_btn].spacing(4).into()
    };

    // Row 3: Horizontal gain fader (spans width)
    let fader = HorizontalFaderWidget::new(track.id, track.gain, track_color);
    let fader_canvas: Element<'_, Message> = canvas(fader)
        .width(Length::Fill)
        .height(Length::Fixed(18.0))
        .into();

    // Row 4: Horizontal VU meter (spans width)
    let (meter_l, meter_r) = input_peaks.unwrap_or(output_peaks);
    let meter_color = if armed {
        th::danger()
    } else if input_peaks.is_some() {
        th::accent()
    } else {
        track_color
    };
    let meter = HorizontalVuMeterWidget::new(meter_l, meter_r, meter_color);
    let meter_canvas: Element<'_, Message> = canvas(meter)
        .width(Length::Fill)
        .height(Length::Fixed(6.0))
        .into();

    let header = column![name_row, track_controls, fader_canvas, meter_canvas]
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
                th::track_bg_selected()
            } else {
                th::bg_surface()
            }
            .into(),
        ),
        ..Default::default()
    });

    // The whole header column selects the track. Child widgets
    // (name, M/S, add, fader) capture their own clicks first,
    // so this only fires on otherwise-dead header space.
    let track_id = track.id;
    mouse_area(card)
        .on_press(Message::select_track(track_id))
        .on_enter(Message::View(ViewMsg::TrackHeaderHovered {
            track_id,
            hovered: true,
        }))
        .on_exit(Message::View(ViewMsg::TrackHeaderHovered {
            track_id,
            hovered: false,
        }))
        .into()
}

fn quiet_action_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: th::text_dim(),
        border: iced::Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::id::TrackId;

    fn audio_track() -> ProjectTrack {
        ProjectTrack::new(TrackId::new(), "Vox".into(), 0)
    }

    #[test]
    fn an_audio_header_shows_its_input_while_hovered_or_in_use() {
        let resting = TrackHeaderFlags::default();
        let hovered = TrackHeaderFlags {
            hovered: true,
            ..resting
        };
        let mut monitoring = audio_track();
        monitoring.input_monitoring = InputMonitoring::On;

        assert!(!resting.shows_audio_input(&audio_track(), false));
        assert!(hovered.shows_audio_input(&audio_track(), false));
        assert!(resting.shows_audio_input(&audio_track(), true));
        assert!(resting.shows_audio_input(&monitoring, false));
    }

    #[test]
    fn hidden_header_actions_take_no_clicks() {
        let resting = TrackHeaderFlags::default();
        let hovered = TrackHeaderFlags {
            hovered: true,
            ..resting
        };
        let automation_open = TrackHeaderFlags {
            automation_open: true,
            ..resting
        };

        assert!(!resting.shows_actions());
        assert!(!resting.shows_automation_toggle());
        assert!(hovered.shows_actions());
        assert!(hovered.shows_automation_toggle());
        assert!(!automation_open.shows_actions());
        assert!(automation_open.shows_automation_toggle());
    }

    #[test]
    fn a_midi_header_never_shows_audio_input() {
        let mut midi = audio_track();
        midi.kind = TrackKind::Midi;
        let hovered = TrackHeaderFlags {
            hovered: true,
            ..TrackHeaderFlags::default()
        };

        assert!(!hovered.shows_audio_input(&midi, true));
    }
}
