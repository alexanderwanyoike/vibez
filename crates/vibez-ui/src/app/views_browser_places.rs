//! Sample-browser places tree rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{button, column, container, horizontal_space, row, scrollable, text};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::icons;
use crate::message::Message;
use crate::state::SampleBrowserMode;
use crate::theme as th;

use super::views_browser_style::*;
use super::*;

impl App {
    pub(super) fn view_browser_places(&self) -> Element<'_, Message> {
        let local_active = self.state.browser.mode == SampleBrowserMode::Local;
        let remote_active = self.state.browser.mode == SampleBrowserMode::Remote;
        let place_button = |label: &'static str, active: bool, mode| {
            button(
                row![
                    text(if active { "●" } else { "○" })
                        .size(9)
                        .color(if active {
                            th::accent()
                        } else {
                            th::text_muted()
                        }),
                    text(label)
                        .size(11)
                        .color(if active { th::text() } else { th::text_dim() })
                ]
                .spacing(7)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::Browser(BrowserMsg::SetSampleBrowserMode(mode)))
            .padding([6, 7])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(active, status))
        };

        let mut places = column![
            text("PLACES").size(9).color(th::text_muted()),
            place_button("Local", local_active, SampleBrowserMode::Local),
        ]
        .spacing(3);

        let all_active = local_active && self.state.browser.current_folder.is_none();
        let all_roots = button(text("All Roots").size(10).color(if all_active {
            th::accent()
        } else {
            th::text_dim()
        }))
        .on_press(Message::Browser(BrowserMsg::SelectLocalFolder(None)))
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |_theme: &Theme, status| browser_place_button_style(all_active, status));
        places = places.push(row![
            horizontal_space().width(Length::Fixed(14.0)),
            all_roots
        ]);

        let mut local_tree_rows = Vec::new();
        for root in &self.state.browser.roots {
            let active = local_active
                && self
                    .state
                    .browser
                    .current_folder
                    .as_ref()
                    .is_some_and(|selected| selected == root);
            let expanded = self.state.browser.expanded_local_folders.contains(root);
            let has_children = self
                .state
                .browser
                .folders
                .iter()
                .any(|folder| folder.root_path == *root && folder.path.parent() == Some(root));
            let label = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string());
            let toggle: Element<'_, Message> = if has_children {
                button(
                    text(if expanded { "▾" } else { "▸" })
                        .size(10)
                        .color(th::text_muted()),
                )
                .on_press(Message::Browser(BrowserMsg::ToggleLocalFolder(
                    root.clone(),
                )))
                .padding([4, 2])
                .style(browser_utility_action_style)
                .into()
            } else {
                container(text("·").size(9).color(th::text_muted()))
                    .padding([4, 3])
                    .into()
            };
            let root_button = button(
                text(label)
                    .size(10)
                    .color(if active { th::accent() } else { th::text_dim() })
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::Browser(BrowserMsg::SelectLocalFolder(Some(
                root.clone(),
            ))))
            .padding([4, 2])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(active, status));
            let remove = button(icons::icon(icons::X).size(8).color(th::text_muted()))
                .on_press(Message::Browser(BrowserMsg::RemoveSampleLibraryRoot(
                    root.clone(),
                )))
                .padding([4, 2])
                .style(browser_utility_action_style);
            let root_state = self.state.browser.root_catalog_label(root);
            let state_marker = text(match root_state {
                "INDEXING" | "UPDATING" => "↻",
                "STALE" | "WATCH ERR" => "!",
                "WARN" => "!",
                _ => "·",
            })
            .size(9)
            .color(if matches!(root_state, "STALE" | "WATCH ERR" | "WARN") {
                th::danger()
            } else {
                th::text_muted()
            });
            local_tree_rows.push(
                row![toggle, root_button, state_marker, remove]
                    .spacing(1)
                    .align_y(iced::Alignment::Center)
                    .into(),
            );
            if expanded {
                self.render_local_places_tree(root, root, 1, &mut local_tree_rows);
            }
        }
        for local_row in local_tree_rows {
            places = places.push(local_row);
        }

        let remote = &self.state.browser.remote;
        let remote_toggle = button(
            text(if remote.place_expanded { "▾" } else { "▸" })
                .size(10)
                .color(th::text_muted()),
        )
        .on_press(Message::Browser(BrowserMsg::ToggleRemotePlace))
        .padding([3, 2])
        .style(browser_utility_action_style);
        let remote_select = button(
            row![
                container(text("")).width(Length::Fixed(1.0)),
                text("Remote")
                    .size(10)
                    .color(if remote_active {
                        th::accent()
                    } else {
                        th::text_dim()
                    })
                    .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Browser(BrowserMsg::SetSampleBrowserMode(
            SampleBrowserMode::Remote,
        )))
        .padding([4, 2])
        .width(Length::Fill)
        .style(move |_theme: &Theme, status| browser_place_button_style(remote_active, status));
        places = places.push(
            row![remote_toggle, remote_select]
                .spacing(1)
                .align_y(iced::Alignment::Center),
        );
        if remote.place_expanded {
            let connection_active = remote_active && remote.current_path.is_empty();
            let connection_toggle = button(
                text(if remote.connection_expanded {
                    "▾"
                } else {
                    "▸"
                })
                .size(10)
                .color(th::text_muted()),
            )
            .on_press(Message::Browser(BrowserMsg::ToggleRemoteConnection))
            .padding([3, 2])
            .style(browser_utility_action_style);
            let connection = button(
                text(remote.catalog.connection_name.as_str())
                    .size(10)
                    .color(if connection_active {
                        th::accent()
                    } else {
                        th::text_dim()
                    })
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::Browser(BrowserMsg::SelectRemoteFolder(
                String::new(),
            )))
            .padding([4, 2])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                browser_place_button_style(connection_active, status)
            });
            places = places.push(
                row![
                    horizontal_space().width(Length::Fixed(REMOTE_CONNECTION_INDENT)),
                    connection_toggle,
                    connection,
                    text(remote.catalog_state.label())
                        .size(8)
                        .color(th::text_muted())
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            );
            if remote.connection_expanded {
                let mut remote_rows = Vec::new();
                self.render_remote_places_tree("", 0, &mut remote_rows);
                for remote_row in remote_rows {
                    places = places.push(remote_row);
                }
            }
        }

        let add = button(
            row![
                icons::icon(icons::PLUS).size(10).color(th::text_muted()),
                text("ADD ROOT").size(9).color(th::text_dim())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddSampleLibraryRoot)
        .padding([4, 5])
        .width(Length::Fill)
        .style(browser_utility_action_style);
        let mut rescan = button(
            row![
                icons::icon(icons::REPEAT).size(10).color(th::text_muted()),
                text("RESCAN").size(9).color(th::text_dim())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        )
        .padding([4, 5])
        .width(Length::Fill)
        .style(browser_utility_action_style);
        if !self.state.browser.roots.is_empty() && !self.state.browser.scan_in_progress {
            rescan = rescan.on_press(Message::RescanSampleLibrary);
        }
        places = places.push(
            column![add, rescan]
                .spacing(4)
                .padding([8, 0])
                .align_x(iced::Alignment::Start),
        );

        container(
            scrollable(container(places.padding(9)).width(Length::Fill)).direction(
                scrollable::Direction::Vertical(scrollable::Scrollbar::default()),
            ),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn render_local_places_tree<'a>(
        &'a self,
        root: &Path,
        parent: &Path,
        depth: usize,
        rows: &mut Vec<Element<'a, Message>>,
    ) {
        let mut children: Vec<_> = self
            .state
            .browser
            .folders
            .iter()
            .filter(|folder| folder.root_path == root && folder.path.parent() == Some(parent))
            .collect();
        children.sort_by_key(|folder| folder.name.to_lowercase());

        for folder in children {
            let expanded = self
                .state
                .browser
                .expanded_local_folders
                .contains(&folder.path);
            let active = self.state.browser.mode == SampleBrowserMode::Local
                && self.state.browser.current_folder.as_ref() == Some(&folder.path);
            let has_children =
                self.state.browser.folders.iter().any(|child| {
                    child.root_path == root && child.path.parent() == Some(&folder.path)
                });
            let toggle: Element<'a, Message> = if has_children {
                button(
                    text(if expanded { "▾" } else { "▸" })
                        .size(10)
                        .color(th::text_muted()),
                )
                .on_press(Message::Browser(BrowserMsg::ToggleLocalFolder(
                    folder.path.clone(),
                )))
                .padding([3, 2])
                .style(browser_utility_action_style)
                .into()
            } else {
                container(text("·").size(9).color(th::text_muted()))
                    .padding([3, 3])
                    .into()
            };
            let select = button(
                text(folder.name.clone())
                    .size(9)
                    .color(if active { th::accent() } else { th::text_dim() })
                    .height(Length::Fixed(12.0))
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::Browser(BrowserMsg::SelectLocalFolder(Some(
                folder.path.clone(),
            ))))
            .padding([3, 1])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(active, status));
            rows.push(
                row![
                    horizontal_space().width(Length::Fixed((depth as f32 * 8.0).min(40.0))),
                    toggle,
                    select
                ]
                .spacing(1)
                .align_y(iced::Alignment::Center)
                .into(),
            );
            if expanded {
                self.render_local_places_tree(root, &folder.path, depth + 1, rows);
            }
        }
    }

    fn render_remote_places_tree<'a>(
        &'a self,
        parent: &str,
        depth: usize,
        rows: &mut Vec<Element<'a, Message>>,
    ) {
        let remote = &self.state.browser.remote;
        for &index in remote.catalog_child_indices(parent) {
            let folder = &remote.catalog.entries[index];
            if !folder.is_folder {
                continue;
            }
            let expanded = self
                .state
                .browser
                .remote
                .expanded
                .contains(&folder.provider_item_id);
            let active = self.state.browser.mode == SampleBrowserMode::Remote
                && self.state.browser.remote.current_path == folder.provider_item_id;
            let has_children = remote
                .catalog_child_indices(&folder.provider_item_id)
                .iter()
                .any(|&child| remote.catalog.entries[child].is_folder);
            let toggle: Element<'a, Message> = if has_children {
                button(
                    text(if expanded { "▾" } else { "▸" })
                        .size(10)
                        .color(th::text_muted()),
                )
                .on_press(Message::Browser(BrowserMsg::ToggleRemoteFolder(
                    folder.provider_item_id.clone(),
                )))
                .padding([3, 2])
                .style(browser_utility_action_style)
                .into()
            } else {
                container(text("·").size(9).color(th::text_muted()))
                    .padding([3, 3])
                    .into()
            };
            let select = button(
                text(folder.name.clone())
                    .size(9)
                    .color(if active { th::accent() } else { th::text_dim() })
                    .height(Length::Fixed(12.0))
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::Browser(BrowserMsg::SelectRemoteFolder(
                folder.provider_item_id.clone(),
            )))
            .padding([3, 1])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(active, status));
            rows.push(
                row![
                    horizontal_space().width(Length::Fixed(remote_places_indent(depth))),
                    toggle,
                    select
                ]
                .spacing(1)
                .align_y(iced::Alignment::Center)
                .into(),
            );
            if expanded {
                self.render_remote_places_tree(&folder.provider_item_id, depth + 1, rows);
            }
        }
    }
}
