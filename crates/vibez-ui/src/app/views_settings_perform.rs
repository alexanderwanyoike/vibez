//! Global Perform input preferences.

use iced::widget::{button, column, container, row, slider, text};
use iced::{Element, Length, Theme};

use crate::domains::perform::{PadPosition, PerformMsg};
use crate::message::Message;
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_settings_perform_tab(&self) -> Element<'_, Message> {
        let title = text("Computer Keys").size(14).color(th::text());
        let hint = text(
            "Select a Pad Position, then press a letter or number key. The mapping is global and applies to every project.",
        )
        .size(11)
        .color(th::text_dim());
        let velocity = self.state.perform.fixed_computer_velocity();
        let velocity_control = column![
            row![
                text("Fixed computer-key velocity")
                    .size(12)
                    .color(th::text()),
                iced::widget::horizontal_space(),
                text(format!("{velocity}")).size(12).color(th::accent())
            ],
            slider(1..=127, velocity, |value| {
                Message::Perform(PerformMsg::SetFixedComputerVelocity(value))
            })
            .step(1_u8),
            text("Applies immediately to Instrument pads and is remembered globally.")
                .size(10)
                .color(th::text_dim())
        ]
        .spacing(5);

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
                    .height(Length::Fixed(58.0))
                    .style(move |_theme: &Theme, status| {
                        let hovered =
                            matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: Some(
                                if waiting || hovered {
                                    th::bg_hover()
                                } else {
                                    th::bg_elevated()
                                }
                                .into(),
                            ),
                            text_color: th::text(),
                            border: iced::Border {
                                color: if waiting { th::accent() } else { th::border() },
                                width: if waiting { 2.0 } else { 1.0 },
                                radius: 5.0.into(),
                            },
                            ..Default::default()
                        }
                    });
                key_row = key_row.push(key_button);
            }
            grid = grid.push(key_row);
        }

        let status = if let Some(position) = self.state.perform.key_rebind_target {
            format!(
                "Waiting for R{} · C{} — press Esc to cancel",
                position.row + 1,
                position.column + 1
            )
        } else {
            "Default physical layout: 1234 / QWER / ASDF / ZXCV".to_string()
        };

        let confirm_enabled = self.state.confirm_project_track_deletion;
        let confirmation = button(
            row![
                crate::icons::icon(if confirm_enabled {
                    crate::icons::CIRCLE_DOT
                } else {
                    crate::icons::CIRCLE
                })
                .size(12)
                .color(if confirm_enabled {
                    th::accent()
                } else {
                    th::text_dim()
                }),
                column![
                    text("Confirm before deleting Project Tracks")
                        .size(12)
                        .color(th::text()),
                    text("Off by default. Track deletion remains available as one Undo step.")
                        .size(10)
                        .color(th::text_dim())
                ]
                .spacing(2)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleProjectTrackDeleteConfirmation)
        .padding([7, 8])
        .width(Length::Fill)
        .style(|_theme: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| th::bg_hover().into()),
            text_color: th::text(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        column![
            title,
            hint,
            velocity_control,
            container(grid).width(Length::Fill).padding([6, 0]),
            text(status).size(10).color(th::text_dim()),
            container(column![])
                .height(Length::Fixed(1.0))
                .width(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }),
            text("Project Tracks").size(13).color(th::text()),
            confirmation
        ]
        .spacing(8)
        .into()
    }
}
