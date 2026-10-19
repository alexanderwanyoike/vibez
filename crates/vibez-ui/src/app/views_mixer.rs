//! Mixer workspace view: channel strips for tracks and buses.
//! Split from views_shell.rs; inherent methods on [`super::App`].

use iced::widget::{button, center, column, container, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::ContextMenuTarget;
use crate::theme as th;
use crate::widgets::mixer_strip::{view_mixer_strip, MixerStripView, StripRole};

use super::*;

/// The desk's own padding round the strips, above and below.
const DESK_PADDING: f32 = 32.0;
/// Returns shown side by side before their area scrolls.
const PINNED_RETURNS: usize = 4;
/// The send bank switch above the strips, when there is more than one bank.
const BANK_BAR_H: f32 = 30.0;

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

        // Strips fill the room they get, but never shrink below a usable
        // fader; on a short window the desk scrolls up and down instead.
        let desk = iced::widget::responsive(move |size| {
            let minimum = crate::widgets::mixer_strip::min_strip_height(
                self.state.project_tracks.buses.len(),
            );
            let bank_bar = self.view_send_bank_bar();
            let bar_h = if bank_bar.is_some() { BANK_BAR_H } else { 0.0 };
            let desk = self.view_desk((size.height - DESK_PADDING - bar_h).max(minimum));
            iced::widget::Scrollable::with_direction(
                column![].push_maybe(bank_bar).push(desk),
                iced::widget::scrollable::Direction::Vertical(
                    iced::widget::scrollable::Scrollbar::new(),
                ),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        });

        mouse_area(
            container(desk)
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

    /// The switch between banks of buses on the send rows; none while every
    /// bus fits one bank.
    fn view_send_bank_bar(&self) -> Option<Element<'_, Message>> {
        use crate::widgets::mixer_strip::{send_banks, SENDS_PER_BANK};
        let banks = send_banks(self.state.project_tracks.buses.len());
        if banks < 2 {
            return None;
        }
        let current = self.state.view.mixer_send_bank.min(banks - 1);
        let mut bar = row![text("SENDS").size(10).color(th::text_dim())]
            .spacing(6)
            .align_y(iced::Alignment::Center);
        for bank in 0..banks {
            let first = bank * SENDS_PER_BANK + 1;
            let active = bank == current;
            bar = bar.push(
                button(
                    text(format!("{first}–{}", first + SENDS_PER_BANK - 1))
                        .size(10)
                        .color(if active { th::accent() } else { th::text_dim() }),
                )
                .on_press(Message::View(ViewMsg::SetMixerSendBank(bank)))
                .padding([3, 10])
                .style(move |_: &Theme, status| button::Style {
                    background: Some(
                        if active || matches!(status, button::Status::Hovered) {
                            th::bg_hover()
                        } else {
                            th::bg_surface()
                        }
                        .into(),
                    ),
                    border: iced::Border {
                        color: if active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }
        Some(
            container(bar)
                .height(BANK_BAR_H)
                .padding([4, 12])
                .center_y(BANK_BAR_H)
                .into(),
        )
    }

    /// The strips, returns and master at `height`.
    fn view_desk(&self, height: f32) -> Element<'_, Message> {
        let playhead_beat = self.state.position_beats();
        let buses = &self.state.project_tracks.buses;
        let mut strips = row![].spacing(4).padding(8);

        for (index, track) in self.state.project_tracks.tracks.iter().enumerate() {
            let selected = self.channel_selected(track.id);
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
                    click: self.channel_click(track.id),
                    number: format!("{:02}", index + 1),
                    editing_name: self.state.view.editing_track_name == Some(track.id),
                    height,
                    send_bank: self.state.view.mixer_send_bank,
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
        let mut returns = row![].spacing(4).padding(8);
        for (index, bus) in buses.iter().enumerate() {
            let selected = self.channel_selected(bus.id);
            returns = returns.push(view_mixer_strip(
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
                    click: self.channel_click(bus.id),
                    number: format!("R{}", index + 1),
                    editing_name: self.state.view.editing_track_name == Some(bus.id),
                    height,
                    send_bank: self.state.view.mixer_send_bank,
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
        let add_bus = container(add_bus_btn)
            .height(Length::Fixed(height))
            .align_y(iced::Alignment::Center);

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
                click: self.channel_click(vibez_core::id::TrackId::MASTER),
                number: "M".into(),
                editing_name: false,
                height,
                send_bank: self.state.view.mixer_send_bank,
                edit_text: &self.state.view.edit_name_text,
                playhead_beat,
                automation: self
                    .state
                    .arrange_content(vibez_core::id::TrackId::MASTER)
                    .map(|content| content.automation.as_slice())
                    .unwrap_or(&[]),
            },
        ))
        .padding(8);

        // Tracks scroll sideways once they outgrow the window; the returns
        // and the master stay pinned on the right like a desk's.
        // The plain wheel scrolls the desk up and down; Shift turns it
        // sideways over the tracks or the returns.
        let channels = crate::widgets::wheel_gate::shift_wheel(
            iced::widget::Scrollable::with_direction(
                strips,
                iced::widget::scrollable::Direction::Horizontal(
                    iced::widget::scrollable::Scrollbar::new(),
                ),
            )
            .width(Length::Fill),
        );
        // Past a few returns their area scrolls on its own, so they never
        // squeeze the tracks off the desk or push the master out of view.
        let returns: Element<'_, Message> = if buses.len() > PINNED_RETURNS {
            let strips = PINNED_RETURNS as f32;
            crate::widgets::wheel_gate::shift_wheel(
                iced::widget::Scrollable::with_direction(
                    returns,
                    iced::widget::scrollable::Direction::Horizontal(
                        iced::widget::scrollable::Scrollbar::new(),
                    ),
                )
                .width(Length::Fixed(
                    strips * crate::widgets::mixer_strip::STRIP_W + (strips - 1.0) * 4.0 + 16.0,
                )),
            )
            .into()
        } else {
            returns.into()
        };
        let mixer_row = row![channels, returns, add_bus, master_strip]
            .spacing(4)
            .padding([8, 4]);

        mixer_row.into()
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
