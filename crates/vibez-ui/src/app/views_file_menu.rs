//! File menu overlay and its Recent Projects submenu.
//!
//! Split out of views_overlays.rs; inherent methods on [`super::App`]. The
//! submenu is aligned to its parent row by arithmetic over fixed row heights,
//! so every row in the menu column MUST carry
//! `.height(Length::Fixed(FILE_MENU_ITEM_HEIGHT))`.

use iced::widget::{
    button, column, container, horizontal_space, mouse_area, row, text, vertical_space,
};
use iced::{Element, Length, Theme};

use crate::domains::project::ProjectMsg;

use crate::icons;
use crate::message::{MenuOverlay, Message};
use crate::theme as th;

use super::*;

const FILE_MENU_ITEM_HEIGHT: f32 = 32.0;
const FILE_MENU_ITEM_SPACING: f32 = 2.0;
const FILE_MENU_CONTENT_PADDING: f32 = 4.0;
const RECENT_PROJECTS_MENU_ITEM_INDEX: usize = 2;

fn file_menu_item_top(item_index: usize) -> f32 {
    FILE_MENU_CONTENT_PADDING + item_index as f32 * (FILE_MENU_ITEM_HEIGHT + FILE_MENU_ITEM_SPACING)
}

impl App {
    pub(super) fn view_file_menu_overlay(&self) -> Element<'_, Message> {
        let make_menu_btn = |label: &'static str, icon: char, msg: Message| {
            button(
                row![
                    icons::icon(icon).size(12).color(th::text()),
                    text(label).size(12).color(th::text())
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::menu_item(MenuOverlay::File, msg))
            .padding([8, 16])
            .height(Length::Fixed(FILE_MENU_ITEM_HEIGHT))
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

        let new_btn = make_menu_btn("New (Empty)", icons::PLUS, Message::NewProject);
        let export_btn = make_menu_btn(
            "Export to WAV...",
            icons::AUDIO_WAVEFORM,
            Message::ExportProject,
        );

        let open_btn = make_menu_btn("Open...", icons::MUSIC, Message::OpenProject);
        let recent_btn = button(
            row![
                icons::icon(icons::MUSIC).size(12).color(th::text()),
                text("Recent Projects").size(12).color(th::text()),
                horizontal_space(),
                text("›")
                    .size(16)
                    .color(if self.state.project.recent_projects_open {
                        th::accent()
                    } else {
                        th::text_dim()
                    }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Project(ProjectMsg::ToggleRecentProjects))
        .padding([8, 16])
        .height(Length::Fixed(FILE_MENU_ITEM_HEIGHT))
        .width(Length::Fill)
        .style(|_theme: &Theme, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => None,
            },
            text_color: th::text(),
            border: iced::Border::default(),
            ..Default::default()
        });
        let save_label = if self.state.project.dirty {
            "Save*"
        } else {
            "Save"
        };
        let save_btn = make_menu_btn(save_label, icons::COPY, Message::SaveProject);
        let save_as_btn = make_menu_btn("Save As...", icons::COPY, Message::SaveProjectAs);
        let settings_btn = button(
            row![
                icons::icon(icons::SLIDERS_VERTICAL)
                    .size(12)
                    .color(th::text()),
                text("Settings...").size(12).color(th::text())
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::menu_item(MenuOverlay::File, Message::OpenSettings))
        .padding([8, 16])
        .height(Length::Fixed(FILE_MENU_ITEM_HEIGHT))
        .width(Length::Fill)
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::text(),
                border: iced::Border::default(),
                ..Default::default()
            }
        });

        let about_btn = make_menu_btn("About vibez", icons::CIRCLE_DOT, Message::OpenAbout);

        let menu_content = column![new_btn]
            .spacing(FILE_MENU_ITEM_SPACING)
            .push(open_btn)
            .push(recent_btn)
            .push(save_btn)
            .push(save_as_btn)
            .push(export_btn)
            .push(settings_btn)
            .push(about_btn)
            .padding(FILE_MENU_CONTENT_PADDING)
            .width(Length::Fixed(220.0));

        let menu_card = container(menu_content).style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        let recent_card = self.state.project.recent_projects_open.then(|| {
            let mut items = column![text("RECENT PROJECTS").size(9).color(th::text_muted())]
                .spacing(2)
                .padding(4)
                .width(Length::Fixed(320.0));
            if self.state.project.recent_project_paths.is_empty() {
                items = items.push(
                    container(text("No recent projects").size(11).color(th::text_dim()))
                        .padding([10, 12]),
                );
            } else {
                for path in &self.state.project.recent_project_paths {
                    let name = path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned();
                    let parent = path
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_default();
                    let open_path = path.clone();
                    items = items.push(
                        button(
                            column![
                                text(name).size(11).color(th::text()),
                                text(parent).size(9).color(th::text_muted()),
                            ]
                            .spacing(1),
                        )
                        .on_press(Message::menu_item(
                            MenuOverlay::File,
                            Message::ProjectOpenPathSelected(Some(open_path)),
                        ))
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(|_theme: &Theme, status| button::Style {
                            background: match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::bg_hover().into())
                                }
                                _ => None,
                            },
                            text_color: th::text(),
                            border: iced::Border::default(),
                            ..Default::default()
                        }),
                    );
                }
                items = items.push(
                    button(
                        row![
                            icons::icon(icons::TRASH_2).size(11).color(th::text_dim()),
                            text("Clear Recent Projects").size(11).color(th::text_dim()),
                        ]
                        .spacing(6),
                    )
                    .on_press(Message::menu_item(
                        MenuOverlay::File,
                        Message::Project(ProjectMsg::ClearRecentProjects),
                    ))
                    .padding([8, 10])
                    .width(Length::Fill)
                    .style(|_theme: &Theme, status| button::Style {
                        background: match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::bg_hover().into())
                            }
                            _ => None,
                        },
                        text_color: th::text_dim(),
                        border: iced::Border::default(),
                        ..Default::default()
                    }),
                );
            }
            container(items).style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
        });

        // Position below the header, near the File button.
        let menus = if let Some(recent_card) = recent_card {
            let aligned_recent_card = column![
                vertical_space().height(Length::Fixed(file_menu_item_top(
                    RECENT_PROJECTS_MENU_ITEM_INDEX
                ))),
                recent_card
            ];
            row![menu_card, aligned_recent_card].spacing(4)
        } else {
            row![menu_card]
        };
        let padded = column![
            vertical_space().height(Length::Fixed(42.0)),
            row![horizontal_space().width(Length::Fixed(60.0)), menus,]
        ];

        mouse_area(container(padded).width(Length::Fill).height(Length::Fill))
            .on_press(Message::dismiss_menu(MenuOverlay::File))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::file_menu_item_top;

    #[test]
    fn file_submenu_starts_at_its_parent_row() {
        assert_eq!(file_menu_item_top(0), 4.0);
        assert_eq!(file_menu_item_top(2), 72.0);
    }
}
