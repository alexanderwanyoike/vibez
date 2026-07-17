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
            x: 400.0,
            y: 300.0,
            target: ContextMenuTarget::ArrangementEmpty,
        }))
        .into()
    }
}
