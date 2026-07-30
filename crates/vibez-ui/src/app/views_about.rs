//! The About dialog: what this build is, and where to find the project.
//!
//! Its own module rather than an addition to `views_settings.rs`, which sits
//! at the crate's 1,000-line ceiling. Dismissal is not implemented here: the
//! backdrop and close button both emit `MenuOverlay::About`, so a press, the
//! Escape key and a menu selection all retire the dialog through the single
//! path in `menu_lifecycle`.

use iced::widget::{button, center, column, container, horizontal_space, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::about;
use crate::icons;
use crate::message::{MenuOverlay, Message};
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_about_modal(&self) -> Element<'_, Message> {
        let close = Message::dismiss_menu(MenuOverlay::About);

        let close_btn = button(icons::icon(icons::X).size(14).color(th::text_dim()))
            .on_press(close)
            .padding([4, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let header = row![
            text(about::APP_NAME).size(22).color(th::accent()),
            horizontal_space(),
            close_btn
        ]
        .align_y(iced::Alignment::Center);

        let version = text(about::build_version_line()).size(13).color(th::text());
        let license = text(format!("Licensed under {}", about::LICENSE))
            .size(12)
            .color(th::text_dim());

        // Rendered as label plus URL so the destination is visible before the
        // click; a browser handoff cannot be previewed or undone.
        let link_btn = |label: &'static str, url: &'static str| {
            button(
                row![
                    text(label)
                        .size(12)
                        .color(th::text())
                        .width(Length::Fixed(84.0)),
                    text(url).size(11).color(th::accent()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::OpenUrl(url))
            .padding([6, 8])
            .width(Length::Fill)
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border::default(),
                    ..Default::default()
                }
            })
        };

        let links = column![
            link_btn("Repository", about::REPOSITORY_URL),
            link_btn("Website", about::WEBSITE_URL),
            link_btn("Releases", about::RELEASES_URL),
        ]
        .spacing(2);

        let divider = container(horizontal_space())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::border().into()),
                ..Default::default()
            });

        let content = column![header, version, license, divider, links]
            .spacing(10)
            .padding(20)
            .width(Length::Fixed(460.0));

        let dialog = container(content).style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        mouse_area(
            container(center(dialog).width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5).into()),
                    ..Default::default()
                }),
        )
        .on_press(Message::dismiss_menu(MenuOverlay::About))
        .into()
    }
}
