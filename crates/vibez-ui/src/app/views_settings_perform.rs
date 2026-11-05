//! Global Perform input preferences.

use iced::widget::{button, column, row, text};
use iced::{Element, Length, Theme};

use crate::domains::perform::{PadPosition, PerformMsg};
use crate::message::Message;
use crate::theme as th;

use super::views_settings_rows::{group, hint, label_block, setting_row, slider_control, switch};
use super::*;

impl App {
    pub(super) fn view_settings_perform_tab(&self) -> Element<'_, Message> {
        let velocity = self.state.perform.fixed_computer_velocity();

        let mut grid = column![].spacing(6);
        for row_index in 0..4 {
            let mut key_row = row![].spacing(6).width(Length::Fill);
            for position in PadPosition::ALL
                .iter()
                .copied()
                .filter(|position| position.row == row_index)
            {
                let waiting = self.state.perform.key_rebind_target == Some(position);
                let key = self.state.perform.input_mapping.key_for(position);
                let label = if waiting {
                    "PRESS KEY".to_string()
                } else {
                    key.label().to_string()
                };
                let content = column![
                    text(format!("R{} · C{}", position.row + 1, position.column + 1))
                        .size(9)
                        .color(th::text_dim()),
                    text(label)
                        .size(if waiting { 10 } else { 15 })
                        .color(if waiting { th::accent() } else { th::text() })
                ]
                .spacing(3)
                .align_x(iced::Alignment::Center);
                let key_button = button(content)
                    .on_press(Message::Perform(PerformMsg::BeginKeyRebind(position)))
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(52.0))
                    .style(move |_theme: &Theme, status| {
                        let style = crate::widgets::control_style::styled(None, status);
                        button::Style {
                            text_color: th::text(),
                            border: iced::Border {
                                color: th::accent(),
                                width: if waiting { 2.0 } else { 0.0 },
                                radius: 7.0.into(),
                            },
                            ..style
                        }
                    });
                key_row = key_row.push(key_button);
            }
            grid = grid.push(key_row);
        }
        let status = if let Some(position) = self.state.perform.key_rebind_target {
            format!(
                "Waiting for R{} · C{}. Press Esc to cancel.",
                position.row + 1,
                position.column + 1
            )
        } else {
            "Default layout: 1234 / QWER / ASDF / ZXCV.".to_string()
        };

        column![
            group([column![
                label_block(
                    "Computer keys",
                    Some(
                        hint(
                            "Choose a pad, then press a letter or number key. The mapping \
                             is global and applies to every project.",
                        )
                        .into(),
                    ),
                ),
                grid,
                hint(status),
            ]
            .spacing(12)
            .into()]),
            group([
                setting_row(
                    "Computer key velocity",
                    Some(hint("How hard computer keys play Instrument pads.").into()),
                    slider_control(
                        1.0..=127.0,
                        f32::from(velocity),
                        1.0,
                        |value| {
                            Message::Perform(PerformMsg::SetFixedComputerVelocity(
                                value.round() as u8
                            ))
                        },
                        velocity.to_string(),
                    ),
                ),
                setting_row(
                    "Confirm before deleting Project Tracks",
                    Some(hint("Off by default. Deleting a track is always one Undo step.").into()),
                    switch(
                        self.state.confirm_project_track_deletion,
                        Message::ToggleProjectTrackDeleteConfirmation,
                    ),
                ),
            ]),
        ]
        .spacing(16)
        .into()
    }
}
