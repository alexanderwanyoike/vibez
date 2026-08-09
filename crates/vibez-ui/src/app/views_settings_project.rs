//! Project persistence preferences.

use iced::widget::{button, column, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_settings_project_tab(&self) -> Element<'_, Message> {
        let enabled = self.state.auto_save_enabled;
        let auto_save = button(
            row![
                crate::icons::icon(if enabled {
                    crate::icons::CIRCLE_DOT
                } else {
                    crate::icons::CIRCLE
                })
                .size(12)
                .color(if enabled { th::accent() } else { th::text_dim() }),
                column![
                    text("Auto save named projects").size(12).color(th::text()),
                    text("On by default. Saves two seconds after editing stops; Untitled projects wait for Save As.")
                        .size(10)
                        .color(th::text_dim())
                ]
                .spacing(2)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleAutoSave)
        .padding([8, 9])
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
            text("Saving").size(14).color(th::text()),
            text("Autosave uses the current project file and never chooses a location for you.")
                .size(11)
                .color(th::text_dim()),
            auto_save,
        ]
        .spacing(9)
        .into()
    }
}
