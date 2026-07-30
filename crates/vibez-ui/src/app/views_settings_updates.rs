//! The Updates settings tab: opt in or out of the startup release
//! check, and see what the last one found.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::update_check;

use super::*;

impl App {
    pub(super) fn view_settings_updates_tab(&self) -> Element<'_, Message> {
        let state = &self.state.update_check;

        let title = text("Updates").size(14).color(th::text());
        let hint = text(
            "Vibez can ask GitHub once a day whether a newer version has been \
             released. It never downloads or installs anything: the notice is a \
             dismissible chip in the status bar with a link to the releases page. \
             Turn this off and no request is made at all.",
        )
        .size(11)
        .color(th::text_dim());

        let toggle_icon = if state.enabled {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::accent())
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::text_dim())
        };
        let toggle_btn = button(
            row![
                toggle_icon,
                text("Check for updates on startup")
                    .size(12)
                    .color(th::text())
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleCheckForUpdates)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::text(),
            border: iced::Border::default(),
            ..Default::default()
        });

        let current = text(format!(
            "You are running version {}.",
            update_check::current_version()
        ))
        .size(11)
        .color(th::text_dim());

        // Report only what the user can act on. A failed check is
        // indistinguishable from "nothing new" by design, so the
        // absence of a version here is never framed as an error.
        let found: Element<'_, Message> = match &state.available {
            Some(version) => row![
                text(format!("Version {version} is available."))
                    .size(11)
                    .color(th::accent()),
                button(text("View releases").size(11).color(th::accent()))
                    .on_press(Message::OpenReleasesPage)
                    .padding([3, 10])
                    .style(|_theme: &Theme, status| {
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::bg_hover().into())
                            }
                            _ => None,
                        };
                        button::Style {
                            background: bg,
                            text_color: th::accent(),
                            border: iced::Border {
                                color: th::accent_dim(),
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    }),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
            None => text("No newer version found.")
                .size(11)
                .color(th::text_muted())
                .into(),
        };

        let divider = container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
            |_theme: &Theme| container::Style {
                background: Some(th::border().into()),
                ..Default::default()
            },
        );

        column![title, hint, toggle_btn, divider, current, found]
            .spacing(10)
            .into()
    }
}
