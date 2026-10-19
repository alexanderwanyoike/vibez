//! Mixer workspace view: channel strips for tracks and buses.
//! Split from views_shell.rs; inherent methods on [`super::App`].

use iced::widget::{button, center, column, container, horizontal_space, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::ContextMenuTarget;
use crate::theme as th;
use crate::widgets::mixer_strip::{view_mixer_strip, MixerStripView, StripRole};

use super::*;

impl App {
    pub(super) fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.project_tracks.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::text_dim());

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    ..Default::default()
                })
                .into();
        }

        // ── Channel strips, buses, pinned master ──
        let playhead_beat = self.state.position_beats();
        let buses = &self.state.project_tracks.buses;
        let mut strips = row![].spacing(4).padding(8).height(Length::Fill);

        for track in &self.state.project_tracks.tracks {
            let selected = self.state.arrangement.selected_track == Some(track.id);
            let strip = view_mixer_strip(
                track,
                StripRole::Track,
                buses,
                MixerStripView {
                    peaks: self
                        .track_meter_peaks
                        .get(&track.id)
                        .copied()
                        .unwrap_or_default(),
                    selected,
                    editing_name: self.state.view.editing_track_name == Some(track.id),
                    edit_text: &self.state.view.edit_name_text,
                    playhead_beat,
                    automation: self
                        .state
                        .arrange_content(track.id)
                        .map(|content| content.automation.as_slice())
                        .unwrap_or(&[]),
                },
            );
            strips = strips.push(strip);
        }

        // Returns live on the right, next to the master: new buses
        // appear where the "+ Bus" pillar sits, growing toward it.
        let mut right_group = row![].spacing(4).padding(8).height(Length::Fill);
        for bus in buses {
            let selected = self.state.arrangement.selected_track == Some(bus.id);
            right_group = right_group.push(view_mixer_strip(
                bus,
                StripRole::Bus,
                buses,
                MixerStripView {
                    peaks: self
                        .track_meter_peaks
                        .get(&bus.id)
                        .copied()
                        .unwrap_or_default(),
                    selected,
                    editing_name: self.state.view.editing_track_name == Some(bus.id),
                    edit_text: &self.state.view.edit_name_text,
                    playhead_beat,
                    automation: self
                        .state
                        .arrange_content(bus.id)
                        .map(|content| content.automation.as_slice())
                        .unwrap_or(&[]),
                },
            ));
        }

        // "+ Bus" pillar: between the last return and the master.
        let add_bus_btn = button(
            column![
                icons::icon(icons::PLUS).size(12).color(th::text_dim()),
                text("Bus").size(9).color(th::text_dim())
            ]
            .spacing(2)
            .align_x(iced::Alignment::Center),
        )
        .on_press(Message::add_bus())
        .padding([12, 6])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => Some(th::bg_surface().into()),
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
        });
        right_group = right_group.push(
            container(add_bus_btn)
                .height(Length::Fill)
                .align_y(iced::Alignment::Center),
        );

        // Master strip — a real channel, pinned to far right
        let master_selected =
            self.state.arrangement.selected_track == Some(vibez_core::id::TrackId::MASTER);
        let master_strip = container(view_mixer_strip(
            &self.state.project_tracks.master,
            StripRole::Master,
            buses,
            MixerStripView {
                peaks: self
                    .track_meter_peaks
                    .get(&vibez_core::id::TrackId::MASTER)
                    .copied()
                    .unwrap_or_default(),
                selected: master_selected,
                editing_name: false,
                edit_text: &self.state.view.edit_name_text,
                playhead_beat,
                automation: self
                    .state
                    .arrange_content(vibez_core::id::TrackId::MASTER)
                    .map(|content| content.automation.as_slice())
                    .unwrap_or(&[]),
            },
        ))
        .padding(8)
        .height(Length::Fill);

        let mixer_row = row![strips, horizontal_space(), right_group, master_strip]
            .spacing(4)
            .padding([8, 4])
            .height(Length::Fill);

        let mixer_content = container(mixer_row)
            .width(Length::Fill)
            .height(Length::Fill);

        mouse_area(
            container(mixer_content)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    ..Default::default()
                }),
        )
        .on_right_press(Message::View(ViewMsg::ShowContextMenu {
            x: self.state.view.cursor_x,
            y: self.state.view.cursor_y,
            target: ContextMenuTarget::ArrangementEmpty,
        }))
        .into()
    }
}

impl App {
    /// What a click on a channel strip does, given the keys held.
    pub(super) fn channel_click(&self, track_id: vibez_core::id::TrackId) -> Message {
        Message::select_track(track_id)
    }
}

#[cfg(test)]
mod channel_selection_tests {
    use std::sync::Arc;

    use iced::keyboard::{Event, Modifiers};
    use vibez_core::id::TrackId;

    use crate::domains::arrangement::ArrangementMsg;
    use crate::message::Message;
    use crate::state::{ProjectTrack, Workspace};

    fn mixer_with(tracks: usize) -> (super::App, Vec<TrackId>) {
        let mut app = super::super::test_support::app();
        let ids: Vec<_> = (0..tracks)
            .map(|index| {
                let track = ProjectTrack::new(TrackId::new(), format!("Track {index}"), 0);
                let id = track.id;
                Arc::make_mut(&mut app.state.project_tracks)
                    .tracks
                    .push(track);
                id
            })
            .collect();
        app.state.view.workspace = Workspace::Mix;
        (app, ids)
    }

    fn hold(app: &mut super::App, modifiers: Modifiers) {
        let _ = app.update(Message::KeyboardInput {
            event: Event::ModifiersChanged(modifiers),
            occurred_at: std::time::Instant::now(),
        });
    }

    #[test]
    fn select_all_in_the_mix_view_selects_every_channel() {
        let (mut app, ids) = mixer_with(3);
        let _ = app.update(Message::SelectAllPressed);
        assert_eq!(app.state.arrangement.selected_channels, ids);
    }

    #[test]
    fn a_strip_click_adds_with_the_command_key_and_ranges_with_shift() {
        let (mut app, ids) = mixer_with(3);
        let command = if super::super::ON_MACOS {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        hold(&mut app, command);
        assert!(matches!(
            app.channel_click(ids[1]),
            Message::Arrangement(ArrangementMsg::ToggleChannelSelection(id)) if id == ids[1]
        ));
        hold(&mut app, Modifiers::SHIFT);
        assert!(matches!(
            app.channel_click(ids[2]),
            Message::Arrangement(ArrangementMsg::SelectChannelRange(id)) if id == ids[2]
        ));
        hold(&mut app, Modifiers::empty());
        assert!(matches!(
            app.channel_click(ids[0]),
            Message::Arrangement(ArrangementMsg::SelectTrack(id)) if id == ids[0]
        ));
    }
}
