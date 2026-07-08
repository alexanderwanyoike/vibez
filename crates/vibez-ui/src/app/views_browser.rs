//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;

use iced::widget::{
    button, column, container, horizontal_space, mouse_area, row, scrollable, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;

use crate::icons;
use crate::message::{BrowserImportTarget, Message};
use crate::state::SampleBrowserEntry;
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_sample_browser_panel(&self) -> Element<'_, Message> {
        let tab_bar = {
            let local_active = matches!(
                self.state.browser.mode,
                crate::state::SampleBrowserMode::Local
            );
            let dropbox_active = !local_active;
            let tab_btn = |label: &'static str, active: bool, mode| {
                button(text(label).size(11).color(if active {
                    th::accent()
                } else {
                    th::text_dim()
                }))
                .on_press(Message::Browser(BrowserMsg::SetSampleBrowserMode(mode)))
                .padding([4, 12])
                .style(move |_theme: &Theme, status| {
                    let bg = if active {
                        Some(th::accent_dim().into())
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
                        text_color: if active { th::accent() } else { th::text_dim() },
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
            };
            row![
                tab_btn(
                    "Local",
                    local_active,
                    crate::state::SampleBrowserMode::Local
                ),
                tab_btn(
                    "Dropbox",
                    dropbox_active,
                    crate::state::SampleBrowserMode::Dropbox,
                ),
            ]
            .spacing(0)
        };

        let body: Element<'_, Message> = match self.state.browser.mode {
            crate::state::SampleBrowserMode::Local => self.view_local_sample_browser(),
            crate::state::SampleBrowserMode::Dropbox => self.view_dropbox_browser(),
        };

        container(
            column![tab_bar, body]
                .spacing(4)
                .padding([4, 0])
                .height(Length::Fill),
        )
        .width(Length::Fixed(320.0))
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
    }

    pub(super) fn view_local_sample_browser(&self) -> Element<'_, Message> {
        let title = text("Sample Browser").size(14).color(th::accent());
        let mut add_root_btn = button(text("Add Root").size(11).color(th::text()))
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_elevated().into()),
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
        add_root_btn = add_root_btn.on_press(Message::AddSampleLibraryRoot);

        let mut rescan_btn = button(text("Rescan").size(11).color(th::text()))
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_elevated().into()),
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
        if !self.state.browser.roots.is_empty() && !self.state.browser.scan_in_progress {
            rescan_btn = rescan_btn.on_press(Message::RescanSampleLibrary);
        }

        let header = row![title, horizontal_space(), add_root_btn, rescan_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        if self.state.browser.roots.is_empty() {
            let empty = column![
                header,
                text("Add a root folder to index your sample library.")
                    .size(12)
                    .color(th::text_dim())
            ]
            .spacing(10)
            .padding(10);
            return container(empty)
                .width(Length::Fixed(320.0))
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_surface().into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                })
                .into();
        }

        let root_label = |path: &PathBuf| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        };

        let mut roots_col = column![].spacing(4);
        let all_active = self.state.browser.root_filter.is_none();
        let mut all_btn = button(text("All Roots").size(11).color(if all_active {
            th::accent()
        } else {
            th::text_dim()
        }))
        .padding([4, 8])
        .style(move |_theme: &Theme, status| {
            let bg = if all_active {
                Some(th::accent_dim().into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_elevated().into()),
                }
            };
            button::Style {
                background: bg,
                text_color: if all_active {
                    th::accent()
                } else {
                    th::text_dim()
                },
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });
        all_btn = all_btn.on_press(Message::Browser(BrowserMsg::SelectSampleBrowserRoot(None)));
        roots_col = roots_col.push(all_btn);

        for root in &self.state.browser.roots {
            let active = self
                .state
                .browser
                .root_filter
                .as_ref()
                .is_some_and(|selected| selected == root);
            let mut filter_btn = button(text(root_label(root)).size(11).color(if active {
                th::accent()
            } else {
                th::text()
            }))
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if active {
                    Some(th::accent_dim().into())
                } else {
                    match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    }
                };
                button::Style {
                    background: bg,
                    text_color: if active { th::accent() } else { th::text() },
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
            filter_btn = filter_btn.on_press(Message::Browser(
                BrowserMsg::SelectSampleBrowserRoot(Some(root.clone())),
            ));

            let remove_btn = button(icons::icon(icons::X).size(10).color(th::danger()))
                .on_press(Message::Browser(BrowserMsg::RemoveSampleLibraryRoot(
                    root.clone(),
                )))
                .padding([3, 6])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::danger(),
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                });

            roots_col = roots_col.push(
                row![filter_btn, remove_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            );
        }

        let search = text_input("Search samples...", &self.state.browser.search)
            .on_input(|s| Message::Browser(BrowserMsg::SampleBrowserSearchChanged(s)))
            .size(12)
            .width(Length::Fill);

        let search_lower = self.state.browser.search.to_lowercase();
        let mut filtered_entries: Vec<&SampleBrowserEntry> = self
            .state
            .browser
            .entries
            .iter()
            .filter(|entry| {
                self.state
                    .browser
                    .root_filter
                    .as_ref()
                    .is_none_or(|root| &entry.root_path == root)
            })
            .filter(|entry| search_lower.is_empty() || entry.search_text.contains(&search_lower))
            .collect();
        filtered_entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        let selected_source = self.state.browser.selected_source.as_ref();
        let selected_entry = self.selected_sample_browser_entry();
        let selected_target = self.selected_browser_device_target();

        let mut entries_col = column![].spacing(2);
        for entry in filtered_entries.iter().take(400) {
            let selected = selected_source.is_some_and(|source| &entry.source == source);
            // mouse_area returns early if its child captures the event, so
            // iced Button underneath would swallow press events. Use a
            // plain container as the click target instead.
            let entry_body = container(
                column![
                    text(entry.name.as_str()).size(12).color(if selected {
                        th::accent()
                    } else {
                        th::text()
                    }),
                    text(entry.relative_path.display().to_string())
                        .size(10)
                        .color(th::text_dim())
                ]
                .spacing(2)
                .width(Length::Fill),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(
                    if selected {
                        th::accent_dim()
                    } else {
                        th::bg_elevated()
                    }
                    .into(),
                ),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
            let entry_dragger: Element<'_, Message> = mouse_area(entry_body)
                .on_press(Message::Browser(BrowserMsg::StartDragSample {
                    source: entry.source.clone(),
                    label: entry.name.clone(),
                }))
                .on_release(Message::ClickLocalBrowserEntry(entry.source.clone()))
                .into();
            let preview_btn = button(icons::icon(icons::VOLUME_2).size(12).color(th::text_dim()))
                .on_press(Message::PreviewLocalEntry(entry.source.clone()))
                .padding([6, 8])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::accent(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                });
            entries_col = entries_col.push(
                row![entry_dragger, preview_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            );
        }

        if filtered_entries.is_empty() {
            entries_col = entries_col.push(
                container(
                    text("No samples match the current filters")
                        .size(11)
                        .color(th::text_dim()),
                )
                .padding([8, 4]),
            );
        }

        let count_label = text(format!(
            "{} shown / {} indexed{}",
            filtered_entries.len().min(400),
            self.state.browser.entries.len(),
            if self.state.browser.scan_in_progress {
                " (scanning...)"
            } else {
                ""
            }
        ))
        .size(10)
        .color(th::text_dim());

        let selected_text = selected_entry
            .map(|entry| entry.relative_path.display().to_string())
            .unwrap_or_else(|| "Select a sample".to_string());
        let selected_hint = match selected_target {
            Some(BrowserImportTarget::Sampler(track_id)) => self
                .state
                .find_track(track_id)
                .map(|track| format!("Load to {}", track.name))
                .unwrap_or_else(|| "Load to sampler".to_string()),
            Some(BrowserImportTarget::DrumRackPad {
                track_id,
                pad_index,
            }) => self
                .state
                .find_track(track_id)
                .map(|track| format!("Load to {} pad {}", track.name, pad_index + 1))
                .unwrap_or_else(|| "Load to drum rack".to_string()),
            _ => "No sampler or drum rack selected".to_string(),
        };

        let mut add_clip_btn = button(text("Add Clip").size(11).color(th::text()))
            .padding([6, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_elevated().into()),
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
        if selected_entry.is_some() {
            add_clip_btn = add_clip_btn.on_press(Message::ImportSelectedBrowserSampleToArrangement);
        }

        let mut load_device_btn = button(text("Load Device").size(11).color(th::text()))
            .padding([6, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_elevated().into()),
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
        if selected_entry.is_some() && selected_target.is_some() {
            load_device_btn = load_device_btn.on_press(Message::LoadSelectedBrowserSampleToDevice);
        }

        let footer = column![
            text(selected_text).size(11).color(th::text()),
            text(selected_hint).size(10).color(th::text_dim()),
            row![add_clip_btn, load_device_btn]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(6);

        column![
            header,
            roots_col,
            search,
            count_label,
            scrollable(entries_col).height(Length::Fill).direction(
                scrollable::Direction::Vertical(scrollable::Scrollbar::default())
            ),
            footer
        ]
        .spacing(8)
        .padding(10)
        .height(Length::Fill)
        .into()
    }

    pub(super) fn view_dropbox_browser(&self) -> Element<'_, Message> {
        let title = text("Dropbox").size(14).color(th::accent());

        if !self.state.browser.dropbox.connected {
            let hint = if self.state.browser.dropbox.auth_in_progress {
                "Waiting for browser authorisation..."
            } else {
                "Connect in Settings > Dropbox to browse your library."
            };
            return column![title, text(hint).size(12).color(th::text_dim())]
                .spacing(10)
                .padding(10)
                .height(Length::Fill)
                .into();
        }

        let account = self
            .state
            .browser
            .dropbox
            .account_email
            .clone()
            .unwrap_or_default();
        let header = column![title, text(account).size(11).color(th::text_dim()),].spacing(2);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        self.render_dropbox_tree(String::new(), 0, &mut rows);
        if rows.is_empty() {
            let msg = if self.state.browser.dropbox.listing_in_progress.contains("") {
                "Listing your Dropbox..."
            } else {
                "Empty (or still fetching)."
            };
            rows.push(text(msg).size(11).color(th::text_dim()).into());
        }
        let mut entries_col = column![].spacing(2);
        for row in rows {
            entries_col = entries_col.push(row);
        }

        let selected_entry = self.selected_dropbox_entry();
        let selected_label = selected_entry
            .as_ref()
            .map(|e| e.path_display.clone())
            .unwrap_or_else(|| "Select a file".to_string());

        // Preview is triggered by click-to-audition on the tree row itself;
        // no dedicated button here.

        let add_clip_btn: Element<'_, Message> = {
            let mut btn = button(text("Add Clip").size(11).color(th::text()))
                .padding([6, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                });
            if let Some(entry) = selected_entry.as_ref().filter(|e| e.is_supported_audio()) {
                btn = btn.on_press(Message::DropboxImportToArrangement(entry.clone()));
            }
            btn.into()
        };

        let load_device_btn: Element<'_, Message> = {
            let mut btn = button(text("Load Device").size(11).color(th::text()))
                .padding([6, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                });
            if let (Some(entry), Some(_)) = (
                selected_entry.as_ref().filter(|e| e.is_supported_audio()),
                self.selected_browser_device_target(),
            ) {
                btn = btn.on_press(Message::DropboxImportToDevice(entry.clone()));
            }
            btn.into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.browser.dropbox.last_error.clone() {
                text(err).size(10).color(th::danger()).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        let footer = column![
            text(selected_label).size(11).color(th::text()),
            row![add_clip_btn, load_device_btn].spacing(6),
            error_line,
        ]
        .spacing(6);

        column![
            header,
            scrollable(entries_col).height(Length::Fill).direction(
                scrollable::Direction::Vertical(scrollable::Scrollbar::default())
            ),
            footer,
        ]
        .spacing(8)
        .padding(10)
        .height(Length::Fill)
        .into()
    }

    pub(super) fn render_dropbox_tree(
        &self,
        path: String,
        depth: usize,
        rows: &mut Vec<Element<'_, Message>>,
    ) {
        let Some(entries) = self.state.browser.dropbox.folders.get(&path) else {
            return;
        };
        let mut sorted: Vec<&DropboxEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| {
            (!a.is_folder, a.name.to_lowercase()).cmp(&(!b.is_folder, b.name.to_lowercase()))
        });
        for entry in sorted {
            let expanded = self
                .state
                .browser
                .dropbox
                .expanded
                .contains(&entry.path_lower);
            let selected =
                self.state.browser.dropbox.selected_path.as_deref() == Some(&entry.path_lower);

            let prefix = if entry.is_folder {
                if expanded {
                    "v "
                } else {
                    "> "
                }
            } else if entry.is_supported_audio() {
                "· "
            } else {
                "  "
            };
            let indent = "  ".repeat(depth);
            let label = format!("{indent}{prefix}{}", entry.name);
            let msg = if entry.is_folder {
                if expanded {
                    Message::Browser(BrowserMsg::DropboxCollapseFolder(entry.path_lower.clone()))
                } else {
                    Message::DropboxExpandFolder(entry.path_lower.clone())
                }
            } else {
                Message::Browser(BrowserMsg::DropboxSelectEntry(entry.clone()))
            };
            if entry.is_supported_audio() {
                // Audio rows use a container + mouse_area so press events
                // reach us (iced Button captures ButtonPressed, which would
                // hide the drag from mouse_area).
                let text_color = if selected { th::accent() } else { th::text() };
                let row_body = container(text(label).size(11).color(text_color))
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(
                            if selected {
                                th::accent_dim()
                            } else {
                                th::bg_elevated()
                            }
                            .into(),
                        ),
                        border: iced::Border::default(),
                        ..Default::default()
                    });
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower.clone(),
                    display_path: entry.path_display.clone(),
                    rev: entry.rev.clone(),
                };
                let dragger: Element<'_, Message> = mouse_area(row_body)
                    .on_press(Message::Browser(BrowserMsg::StartDragSample {
                        source,
                        label: entry.name.clone(),
                    }))
                    .on_release(msg)
                    .into();
                let speaker = button(icons::icon(icons::VOLUME_2).size(11).color(th::accent()))
                    .on_press(Message::DropboxPreview(entry.clone()))
                    .padding([3, 6])
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
                            border: iced::Border::default(),
                            ..Default::default()
                        }
                    });
                rows.push(
                    row![dragger, speaker]
                        .spacing(2)
                        .align_y(iced::Alignment::Center)
                        .into(),
                );
            } else {
                // Folders + non-audio entries keep the button path since they
                // don't participate in drag.
                let btn = button(text(label).size(11).color(if selected {
                    th::accent()
                } else if entry.is_folder {
                    th::text()
                } else {
                    th::text_dim()
                }))
                .on_press(msg)
                .padding([3, 6])
                .width(Length::Fill)
                .style(move |_theme: &Theme, status| {
                    let bg = if selected {
                        Some(th::accent_dim().into())
                    } else {
                        match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::bg_hover().into())
                            }
                            _ => Some(th::bg_elevated().into()),
                        }
                    };
                    button::Style {
                        background: bg,
                        text_color: if selected { th::accent() } else { th::text() },
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                });
                rows.push(btn.into());
            }

            if entry.is_folder && expanded {
                self.render_dropbox_tree(entry.path_lower.clone(), depth + 1, rows);
            }
        }
    }
}
