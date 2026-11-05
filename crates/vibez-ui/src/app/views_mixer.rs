//! Mixer workspace view: channel strips for tracks and buses.
//! Split from views_shell.rs; inherent methods on [`super::App`].

use iced::widget::{button, center, column, container, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::ContextMenuTarget;
use crate::theme as th;
use crate::widgets::control_style;
use crate::widgets::mixer_strip::{strip_height, view_mixer_strip, MixerStripView, StripRole};
use crate::widgets::quiet_scroll;

use super::*;

/// The desk's padding round the strips, above and below.
const DESK_PADDING: f32 = 16.0;
const DESK_TOP: f32 = 10.0;
const DESK_SIDE: f32 = 10.0;
/// Space between neighbouring strips.
const STRIP_GAP: f32 = 8.0;
/// The send bank switch above the strips, when there is more than one bank.
const BANK_BAR_H: f32 = 34.0;

impl App {
    pub(super) fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.project_tracks.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::text_dim());
            return center(prompt)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // On a window too short for the whole desk it scrolls up and down,
        // so no strip is ever squashed.
        let desk = column![]
            .push_maybe(self.view_send_bank_bar())
            .push(self.view_desk());
        mouse_area(
            quiet_scroll::vertical(desk)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_right_press(Message::View(ViewMsg::ShowContextMenu {
            x: self.state.view.cursor_x,
            y: self.state.view.cursor_y,
            target: ContextMenuTarget::ArrangementEmpty,
        }))
        .into()
    }

    /// The whole desk's height: the bank switch when shown, then the strips.
    pub(super) fn mix_desk_height(&self) -> f32 {
        let buses = self.state.project_tracks.buses.len();
        let bank_bar = if crate::widgets::mixer_strip::send_banks(buses) > 1 {
            BANK_BAR_H
        } else {
            0.0
        };
        bank_bar + DESK_PADDING + strip_height(buses)
    }

    /// The Mix workspace is as tall as its desk, leaving the detail panel
    /// the rest, but never squeezes the detail panel below its minimum.
    pub(super) fn mix_workspace_height(&self) -> f32 {
        let chrome = match super::views_control_bar::control_bar_rows(self.state.view.window_width)
        {
            super::views_control_bar::ControlBarRows::One => 1.0,
            super::views_control_bar::ControlBarRows::Two => 2.0,
        } * super::views_control_bar_style::BAR_HEIGHT;
        let room = self.state.view.window_height
            - chrome
            - crate::shell_layout::STATUS_HEIGHT
            - 3.0 * crate::shell_layout::PANE_GAP
            - crate::shell_layout::DETAIL_PANEL_MIN_HEIGHT;
        self.mix_desk_height().min(room).max(0.0)
    }

    /// The switch between banks of buses on the send rows; none while every
    /// bus fits one bank.
    fn view_send_bank_bar(&self) -> Option<Element<'_, Message>> {
        use crate::widgets::mixer_strip::{send_banks, SENDS_PER_BANK};
        let banks = send_banks(self.state.project_tracks.buses.len());
        if banks < 2 {
            return None;
        }
        let current = self.state.view.mixer_send_bank.min(banks - 1);
        let segments = row((0..banks).map(|bank| {
            let first = bank * SENDS_PER_BANK + 1;
            button(text(format!("{first}–{}", first + SENDS_PER_BANK - 1)).size(10.5))
                .on_press(Message::View(ViewMsg::SetMixerSendBank(bank)))
                .padding([3, 8])
                .style(control_style::style((bank == current).then(th::accent)))
                .into()
        }))
        .spacing(2);
        Some(
            container(
                row![
                    text("SENDS")
                        .font(crate::typography::PERFORM_LABEL)
                        .size(8)
                        .color(th::text_muted()),
                    segments
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding {
                top: DESK_TOP,
                left: DESK_SIDE + 4.0,
                ..iced::Padding::ZERO
            })
            .height(BANK_BAR_H)
            .into(),
        )
    }

    fn view_desk_strip<'a>(
        &'a self,
        track: &'a crate::state::ProjectTrack,
        role: StripRole,
        number: String,
        height: f32,
    ) -> Element<'a, Message> {
        let id = track.id;
        view_mixer_strip(
            track,
            role,
            &self.state.project_tracks.buses,
            MixerStripView {
                peaks: self.track_meter_peaks.get(&id).copied().unwrap_or_default(),
                selected: if role == StripRole::Master {
                    self.state.arrangement.selected_track == Some(id)
                } else {
                    self.channel_selected(id)
                },
                click: self.channel_click(id),
                number,
                editing_name: role != StripRole::Master
                    && self.state.view.editing_track_name == Some(id),
                height,
                send_bank: self.state.view.mixer_send_bank,
                edit_text: &self.state.view.edit_name_text,
                playhead_beat: self.state.transport.position_beats(),
                automation: self
                    .state
                    .arrange_content(id)
                    .map(|content| content.automation.as_slice())
                    .unwrap_or(&[]),
            },
        )
    }

    /// The tracks then the returns in one row that scrolls sideways, with
    /// the master pinned on the right.
    fn view_desk(&self) -> Element<'_, Message> {
        let buses = &self.state.project_tracks.buses;
        let height = strip_height(buses.len());
        let strip = |track, role, number| self.view_desk_strip(track, role, number, height);
        let mut channels = row![].spacing(STRIP_GAP);
        for (index, track) in self.state.project_tracks.tracks.iter().enumerate() {
            channels = channels.push(strip(track, StripRole::Track, format!("{:02}", index + 1)));
        }
        // Returns follow the tracks, and new buses appear where the "+ Bus"
        // pillar sits, growing toward the master.
        for (index, bus) in buses.iter().enumerate() {
            channels = channels.push(strip(bus, StripRole::Bus, format!("R{}", index + 1)));
        }
        let add_bus_btn = button(
            column![icons::icon(icons::PLUS).size(12), text("Bus").size(9)]
                .spacing(2)
                .align_x(iced::Alignment::Center),
        )
        .on_press(Message::add_bus())
        .padding([12, 6])
        .style(control_style::style(None));
        channels = channels.push(container(add_bus_btn).center_y(Length::Fixed(height)));

        // The plain wheel scrolls the desk up and down; Shift turns it
        // sideways over the channels.
        let channels = crate::widgets::wheel_gate::shift_wheel(
            quiet_scroll::horizontal(container(channels).padding(iced::Padding {
                bottom: DESK_PADDING - DESK_TOP,
                ..iced::Padding::ZERO
            }))
            .width(Length::Fill),
        );
        let master = container(strip(
            &self.state.project_tracks.master,
            StripRole::Master,
            "M".into(),
        ))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            shadow: iced::Shadow {
                color: th::with_alpha(iced::Color::BLACK, if th::is_light() { 0.15 } else { 0.45 }),
                offset: iced::Vector::new(-6.0, 0.0),
                blur_radius: 14.0,
            },
            border: iced::Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        row![channels, master]
            .spacing(STRIP_GAP)
            .padding(iced::Padding {
                top: DESK_TOP,
                right: DESK_SIDE,
                bottom: 0.0,
                left: DESK_SIDE,
            })
            .into()
    }
}

impl App {
    /// What a click on a channel strip does, given the keys held: the
    /// platform command key adds or removes the channel, Shift selects the
    /// range up to it.
    pub(super) fn channel_click(&self, track_id: vibez_core::id::TrackId) -> Message {
        use crate::domains::arrangement::ArrangementMsg;
        let modifiers = self.state.view.held_modifiers;
        if super::command_held(modifiers, super::ON_MACOS) {
            Message::Arrangement(ArrangementMsg::ToggleChannelSelection(track_id))
        } else if modifiers.shift() {
            Message::Arrangement(ArrangementMsg::SelectChannelRange(track_id))
        } else {
            Message::select_track(track_id)
        }
    }

    /// Whether a strip shows as selected: one of the set, or the selected
    /// channel when no set has been made.
    fn channel_selected(&self, track_id: vibez_core::id::TrackId) -> bool {
        let set = &self.state.arrangement.selected_channels;
        set.contains(&track_id)
            || (set.is_empty() && self.state.arrangement.selected_track == Some(track_id))
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

#[cfg(test)]
mod desk_fit_tests {
    use super::{strip_height, DESK_PADDING};
    use crate::shell_layout::{PANE_GAP, STATUS_HEIGHT};

    #[test]
    fn the_mix_workspace_is_as_tall_as_its_desk_until_the_detail_panel_needs_the_room() {
        let mut app = super::super::test_support::app();
        app.state.view.window_width = 1440.0;
        app.state.view.window_height = 900.0;
        assert_eq!(app.mix_workspace_height(), app.mix_desk_height());

        app.state.view.window_height = 600.0;
        // Below the chrome row, the Mix card, the gap above the detail card,
        // the detail card, and the gap above the status bar.
        let detail = 600.0
            - super::super::views_control_bar_style::BAR_HEIGHT
            - app.mix_workspace_height()
            - PANE_GAP
            - PANE_GAP
            - STATUS_HEIGHT;
        assert_eq!(detail, crate::shell_layout::DETAIL_PANEL_MIN_HEIGHT);
    }

    #[test]
    fn a_whole_desk_fits_the_smallest_window_above_the_detail_panel() {
        let window = 760.0;
        let room = window
            - super::super::views_control_bar_style::BAR_HEIGHT
            - STATUS_HEIGHT
            - 3.0 * PANE_GAP
            - crate::shell_layout::DETAIL_PANEL_MIN_HEIGHT;
        assert!(DESK_PADDING + strip_height(2) <= room);
    }
}
