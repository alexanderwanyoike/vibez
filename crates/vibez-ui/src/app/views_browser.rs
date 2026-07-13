//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, column, container, horizontal_space, mouse_area, row, scrollable, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;

use crate::icons;
use crate::message::Message;
use crate::state::{BrowserDockLayout, SampleBrowserEntry, SampleBrowserMode};
use crate::theme as th;

use super::*;

impl App {
    pub(super) fn view_sample_browser_panel(&self) -> Element<'_, Message> {
        let width = self
            .state
            .browser
            .effective_dock_width(self.state.view.window_width);
        let layout = self.state.browser.dock_layout(self.state.view.window_width);
        let layout_label = match layout {
            BrowserDockLayout::Narrow => "NARROW",
            BrowserDockLayout::Standard => "STANDARD",
            BrowserDockLayout::Wide => "WIDE",
        };

        let width_down = button(text("−").size(14).color(th::text_dim()))
            .on_press(Message::Browser(BrowserMsg::NudgeDockWidth(-40.0)))
            .padding([2, 7])
            .style(browser_icon_button_style);
        let width_up = button(text("+").size(13).color(th::text_dim()))
            .on_press(Message::Browser(BrowserMsg::NudgeDockWidth(40.0)))
            .padding([2, 7])
            .style(browser_icon_button_style);
        let close = button(icons::icon(icons::X).size(11).color(th::text_dim()))
            .on_press(Message::Browser(BrowserMsg::ToggleSampleBrowser))
            .padding([3, 6])
            .style(browser_icon_button_style);

        let title_row = row![
            text("BROWSER").size(12).color(th::text()),
            text(layout_label).size(9).color(th::text_muted()),
            horizontal_space(),
            width_down,
            width_up,
            close
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center);

        let search = text_input("Search this location…", &self.state.browser.search)
            .on_input(|value| Message::Browser(BrowserMsg::SampleBrowserSearchChanged(value)))
            .size(12)
            .padding([7, 9])
            .width(Length::Fill);

        let body: Element<'_, Message> = match self.state.browser.mode {
            SampleBrowserMode::Local => self.view_local_sample_browser(),
            SampleBrowserMode::Dropbox => self.view_dropbox_browser(),
        };

        let content: Element<'_, Message> = if layout == BrowserDockLayout::Wide {
            row![
                container(self.view_browser_places())
                    .width(Length::Fixed(158.0))
                    .height(Length::Fill)
                    .style(browser_places_style),
                body
            ]
            .height(Length::Fill)
            .into()
        } else {
            let place_name = match self.state.browser.mode {
                SampleBrowserMode::Local => "Local  /  All Roots",
                SampleBrowserMode::Dropbox => "Remote  /  Alex's Dropbox",
            };
            let disclosure = if self.state.browser.places_drawer_open {
                icons::CHEVRON_UP
            } else {
                icons::CHEVRON_DOWN
            };
            let location = button(
                row![
                    text(place_name).size(11).color(th::text()),
                    horizontal_space(),
                    icons::icon(disclosure).size(10).color(th::text_dim())
                ]
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::Browser(BrowserMsg::TogglePlacesDrawer))
            .padding([6, 9])
            .width(Length::Fill)
            .style(browser_location_button_style);
            let mut stack = column![location].spacing(4).height(Length::Fill);
            if self.state.browser.places_drawer_open {
                stack = stack.push(
                    container(self.view_browser_places())
                        .width(Length::Fill)
                        .style(browser_places_style),
                );
            }
            stack.push(body).into()
        };

        container(
            column![
                container(column![title_row, search].spacing(7))
                    .padding([8, 10])
                    .style(browser_header_style),
                content,
                self.view_browser_audition_footer(layout)
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(width))
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

    pub(super) fn view_browser_splitter(&self) -> Element<'_, Message> {
        mouse_area(
            container(text(""))
                .width(Length::Fixed(7.0))
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(if self.state.browser.dock_resize_active {
                        th::accent_dim().into()
                    } else {
                        th::divider().into()
                    }),
                    ..Default::default()
                }),
        )
        .on_press(Message::Browser(BrowserMsg::BeginDockResize))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
    }

    fn view_browser_places(&self) -> Element<'_, Message> {
        let local_active = self.state.browser.mode == SampleBrowserMode::Local;
        let remote_active = self.state.browser.mode == SampleBrowserMode::Dropbox;
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

        let all_active = local_active && self.state.browser.root_filter.is_none();
        let all_roots = button(text("All Roots").size(10).color(if all_active {
            th::accent()
        } else {
            th::text_dim()
        }))
        .on_press(Message::Browser(BrowserMsg::SelectSampleBrowserRoot(None)))
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |_theme: &Theme, status| browser_place_button_style(all_active, status));
        places = places.push(row![
            horizontal_space().width(Length::Fixed(14.0)),
            all_roots
        ]);

        for root in &self.state.browser.roots {
            let active = local_active
                && self
                    .state
                    .browser
                    .root_filter
                    .as_ref()
                    .is_some_and(|selected| selected == root);
            let label = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string());
            let root_button = button(text(label).size(10).color(if active {
                th::accent()
            } else {
                th::text_dim()
            }))
            .on_press(Message::Browser(BrowserMsg::SelectSampleBrowserRoot(Some(
                root.clone(),
            ))))
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| browser_place_button_style(active, status));
            places = places.push(row![
                horizontal_space().width(Length::Fixed(14.0)),
                root_button
            ]);
        }

        places = places.push(place_button(
            "Remote",
            remote_active,
            SampleBrowserMode::Dropbox,
        ));
        let connection_color = if self.state.browser.dropbox.connected {
            th::text_dim()
        } else {
            th::text_muted()
        };
        places = places.push(row![
            horizontal_space().width(Length::Fixed(22.0)),
            container(
                text(if self.state.browser.dropbox.connected {
                    "Alex's Dropbox"
                } else {
                    "Dropbox · disconnected"
                })
                .size(10)
                .color(connection_color),
            )
            .padding([3, 0]),
        ]);

        let add = button(text("+ Root").size(10).color(th::text_dim()))
            .on_press(Message::AddSampleLibraryRoot)
            .padding([4, 7])
            .style(browser_icon_button_style);
        let mut rescan = button(text("Rescan").size(10).color(th::text_dim()))
            .padding([4, 7])
            .style(browser_icon_button_style);
        if !self.state.browser.roots.is_empty() && !self.state.browser.scan_in_progress {
            rescan = rescan.on_press(Message::RescanSampleLibrary);
        }
        places = places.push(
            row![add, rescan]
                .spacing(4)
                .padding([8, 0])
                .align_y(iced::Alignment::Center),
        );

        container(places.padding(9)).width(Length::Fill).into()
    }

    fn view_browser_audition_footer(&self, layout: BrowserDockLayout) -> Element<'_, Message> {
        let selected_local = self.selected_sample_browser_entry();
        let selected_dropbox = self.selected_dropbox_entry();
        let selected_label = match self.state.browser.mode {
            SampleBrowserMode::Local => selected_local
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| "No source selected".into()),
            SampleBrowserMode::Dropbox => selected_dropbox
                .as_ref()
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| "No source selected".into()),
        };

        let preview_message = match self.state.browser.mode {
            SampleBrowserMode::Local => {
                selected_local.map(|entry| Message::PreviewLocalEntry(entry.source.clone()))
            }
            SampleBrowserMode::Dropbox => selected_dropbox
                .as_ref()
                .filter(|entry| entry.is_supported_audio())
                .map(|entry| Message::DropboxPreview(entry.clone())),
        };
        let mut play = button(icons::icon(icons::PLAY).size(12).color(th::text_dim()))
            .padding([6, 8])
            .style(browser_transport_button_style);
        if let Some(message) = preview_message {
            play = play.on_press(message);
        }
        let stop = button(icons::icon(icons::STOP).size(11).color(th::text_dim()))
            .on_press(Message::StopBrowserPreview)
            .padding([6, 8])
            .style(browser_transport_button_style);

        let waveform = container(text("▁▂▄▇▅▂▃▆▄▁▃▇▆▂▁").size(13).color(th::waveform()))
            .padding([5, 7])
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::display_bg().into()),
                border: iced::Border {
                    color: th::divider(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });

        let controls = row![
            play,
            stop,
            text("RAW").size(9).color(th::text_dim()),
            waveform
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center);
        let import_message = match self.state.browser.mode {
            SampleBrowserMode::Local => {
                selected_local.map(|_| Message::ImportSelectedBrowserSampleToArrangement)
            }
            SampleBrowserMode::Dropbox => selected_dropbox
                .as_ref()
                .filter(|entry| entry.is_supported_audio())
                .map(|entry| Message::DropboxImportToArrangement(entry.clone())),
        };
        let device_message = match self.state.browser.mode {
            SampleBrowserMode::Local => (selected_local.is_some()
                && self.selected_browser_device_target().is_some())
            .then_some(Message::LoadSelectedBrowserSampleToDevice),
            SampleBrowserMode::Dropbox => selected_dropbox
                .as_ref()
                .filter(|entry| entry.is_supported_audio())
                .filter(|_| self.selected_browser_device_target().is_some())
                .map(|entry| Message::DropboxImportToDevice(entry.clone())),
        };
        let mut arrange = button(text("Add to Arrange").size(10).color(th::text_dim()))
            .padding([4, 7])
            .style(browser_transport_button_style);
        if let Some(message) = import_message {
            arrange = arrange.on_press(message);
        }
        let mut device = button(text("Load Device").size(10).color(th::text_dim()))
            .padding([4, 7])
            .style(browser_transport_button_style);
        if let Some(message) = device_message {
            device = device.on_press(message);
        }
        let import_controls = row![arrange, device].spacing(5);
        let contents: Element<'_, Message> = if layout == BrowserDockLayout::Narrow {
            column![
                row![
                    text("AUDITION").size(9).color(th::text_muted()),
                    horizontal_space(),
                    text(selected_label).size(10).color(th::text_dim())
                ]
                .align_y(iced::Alignment::Center),
                controls
            ]
            .spacing(5)
            .into()
        } else {
            column![
                row![
                    text("AUDITION").size(9).color(th::text_muted()),
                    text(selected_label).size(11).color(th::text()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                controls,
                import_controls
            ]
            .spacing(5)
            .into()
        };

        container(contents)
            .padding([7, 9])
            .width(Length::Fill)
            .style(browser_footer_style)
            .into()
    }

    pub(super) fn view_local_sample_browser(&self) -> Element<'_, Message> {
        if self.state.browser.roots.is_empty() {
            let add = button(text("Add Local Root").size(11).color(th::text()))
                .on_press(Message::AddSampleLibraryRoot)
                .padding([6, 10])
                .style(browser_transport_button_style);
            return container(
                column![
                    text("RESULTS").size(9).color(th::text_muted()),
                    text("Your Local Place is empty").size(13).color(th::text()),
                    text("Choose a folder to begin indexing supported audio.")
                        .size(11)
                        .color(th::text_dim()),
                    add
                ]
                .spacing(8)
                .padding(12),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(browser_results_style)
            .into();
        }

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
        let mut entries_col = column![].spacing(1);
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
            let preview_btn = button(icons::icon(icons::PLAY).size(11).color(th::text_dim()))
                .on_press(Message::PreviewLocalEntry(entry.source.clone()))
                .padding([6, 8])
                .style(browser_transport_button_style);
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

        let count = format!(
            "{} shown / {} indexed{}",
            filtered_entries.len().min(400),
            self.state.browser.entries.len(),
            if self.state.browser.scan_in_progress {
                " (scanning...)"
            } else {
                ""
            }
        );

        container(
            column![
                row![
                    text("RESULTS").size(9).color(th::text_muted()),
                    horizontal_space(),
                    text(count).size(9).color(th::text_dim())
                ]
                .align_y(iced::Alignment::Center),
                scrollable(entries_col).height(Length::Fill).direction(
                    scrollable::Direction::Vertical(scrollable::Scrollbar::default())
                )
            ]
            .spacing(6)
            .padding(8)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(browser_results_style)
        .into()
    }

    pub(super) fn view_dropbox_browser(&self) -> Element<'_, Message> {
        if !self.state.browser.dropbox.connected {
            let hint = if self.state.browser.dropbox.auth_in_progress {
                "Waiting for browser authorisation..."
            } else {
                "Connect in Settings > Dropbox to browse your library."
            };
            return container(
                column![
                    text("RESULTS").size(9).color(th::text_muted()),
                    text("Remote Connection unavailable")
                        .size(13)
                        .color(th::text()),
                    text(hint).size(11).color(th::text_dim())
                ]
                .spacing(8)
                .padding(12),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(browser_results_style)
            .into();
        }

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
        let status: Element<'_, Message> =
            if let Some(error) = self.state.browser.dropbox.last_error.clone() {
                text(format!("Error · {error}"))
                    .size(9)
                    .color(th::danger())
                    .into()
            } else {
                let account = self
                    .state
                    .browser
                    .dropbox
                    .account_email
                    .clone()
                    .unwrap_or_else(|| "Connected".into());
                text(account).size(9).color(th::text_dim()).into()
            };

        container(
            column![
                row![
                    text("RESULTS").size(9).color(th::text_muted()),
                    horizontal_space(),
                    status
                ]
                .align_y(iced::Alignment::Center),
                scrollable(entries_col).height(Length::Fill).direction(
                    scrollable::Direction::Vertical(scrollable::Scrollbar::default())
                )
            ]
            .spacing(6)
            .padding(8)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(browser_results_style)
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

fn browser_icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: th::text_dim(),
        border: iced::Border {
            color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::border()
            },
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn browser_location_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::divider()
            },
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn browser_place_button_style(active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if active {
                th::accent_dim()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                iced::Color::TRANSPARENT
            }
            .into(),
        ),
        text_color: if active { th::text() } else { th::text_dim() },
        border: iced::Border {
            color: if active {
                th::accent()
            } else {
                iced::Color::TRANSPARENT
            },
            width: if active { 1.0 } else { 0.0 },
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn browser_transport_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text_dim(),
        border: iced::Border {
            color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::border()
            },
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

fn browser_header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_places_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_dark().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_results_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_footer_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}
