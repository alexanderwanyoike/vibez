//! Appearance settings: theme selection and interface scale.
//!
//! Split out of `views_settings.rs`, which had reached the 1,000-line
//! ceiling this repository enforces.

use iced::widget::{button, column, container, horizontal_space, row, text, text_input};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_settings_appearance_tab(&self) -> Element<'_, Message> {
        use iced::widget::scrollable;

        let title = text("Appearance").size(14).color(th::text());
        let hint = text(
            "Themes recolor the whole interface, track palette included. \
             Drop .vzt files in the themes folder and rescan to add your own.",
        )
        .size(11)
        .color(th::text_dim());

        // One selectable row per theme: swatch strip + name.
        let theme_row = |palette: &th::ThemePalette| -> Element<'_, Message> {
            let name = palette.name.clone();
            let selected = self.state.current_theme_name == name;
            let mut swatches = row![].spacing(2);
            for color in [
                palette.bg_dark,
                palette.bg_elevated,
                palette.accent,
                palette.track_colors[0],
                palette.track_colors[3],
                palette.track_colors[5],
            ] {
                swatches = swatches.push(
                    container(
                        column![]
                            .width(Length::Fixed(14.0))
                            .height(Length::Fixed(14.0)),
                    )
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(color.into()),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }
            let label =
                text(name.clone())
                    .size(12)
                    .color(if selected { th::accent() } else { th::text() });
            let marker: Element<'_, Message> = if selected {
                icons::icon(icons::CIRCLE_DOT)
                    .size(11)
                    .color(th::accent())
                    .into()
            } else {
                icons::icon(icons::CIRCLE)
                    .size(11)
                    .color(th::text_muted())
                    .into()
            };
            button(
                row![marker, swatches, label]
                    .spacing(10)
                    .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SelectTheme(name))
            .padding([5, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if selected {
                    Some(th::bg_elevated().into())
                } else {
                    match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    }
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: if selected {
                            th::accent_dim()
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
        };

        let mut builtin_col = column![].spacing(2);
        for palette in crate::themes::builtins() {
            builtin_col = builtin_col.push(theme_row(&palette));
        }

        // User theme section: scanned .vzt files + rescan, like the
        // plugin library.
        let user_header = row![
            text("Your Themes").size(13).color(th::text()),
            horizontal_space(),
            text(crate::themes::themes_dir().display().to_string())
                .size(10)
                .color(th::text_muted()),
            button(text("Rescan").size(11).color(th::accent()))
                .on_press(Message::RescanThemes)
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
        .align_y(iced::Alignment::Center);

        let mut user_col = column![].spacing(2);
        if self.state.user_themes.is_empty() {
            user_col = user_col.push(
                text("No .vzt themes found: save one below or drop files in the folder.")
                    .size(11)
                    .color(th::text_muted()),
            );
        } else {
            for user in &self.state.user_themes {
                user_col = user_col.push(theme_row(&user.palette));
            }
        }

        // Save the active palette under a new name.
        let save_row = row![
            text_input("Theme name...", &self.state.theme_save_name)
                .on_input(Message::ThemeSaveNameChanged)
                .on_submit(Message::SaveCurrentTheme)
                .size(12)
                .padding([5, 8]),
            button(text("Save Current").size(11).color(th::text()))
                .on_press(Message::SaveCurrentTheme)
                .padding([6, 12])
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
        .align_y(iced::Alignment::Center);

        let divider = || {
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                },
            )
        };

        let list = scrollable(column![builtin_col, divider(), user_header, user_col,].spacing(8))
            .height(Length::Fixed(300.0));

        column![title, hint, list, divider(), save_row]
            .spacing(10)
            .into()
    }
}
