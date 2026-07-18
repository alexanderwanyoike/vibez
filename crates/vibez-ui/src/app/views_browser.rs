//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, column, container, horizontal_space, mouse_area, row, scrollable, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::icons;
use crate::message::Message;
use crate::state::SampleBrowserMode;
use crate::theme as th;

use super::views_browser_style::*;
use super::*;

impl App {
    pub(super) fn view_sample_browser_panel(&self) -> Element<'_, Message> {
        let width = self
            .state
            .browser
            .effective_dock_width(self.state.view.window_width);
        let places_width = self
            .state
            .browser
            .places_pane_width(self.state.view.window_width);

        let close = button(icons::icon(icons::X).size(11).color(th::text_dim()))
            .on_press(Message::Browser(BrowserMsg::ToggleSampleBrowser))
            .padding([3, 6])
            .style(browser_icon_button_style);

        let title_row = row![
            text("BROWSER").size(12).color(th::text()),
            horizontal_space(),
            close
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center);

        let search = text_input("Search this location…", &self.state.browser.search)
            .on_input(|value| Message::Browser(BrowserMsg::SampleBrowserSearchChanged(value)))
            .size(12)
            .padding([7, 9])
            .width(Length::Fill)
            .style(|_theme: &Theme, _status| iced::widget::text_input::Style {
                background: th::bg_dark().into(),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                icon: th::text_dim(),
                placeholder: th::text_dim(),
                value: th::text(),
                selection: th::accent(),
            });

        let search_context: Element<'_, Message> = match self.state.browser.mode {
            SampleBrowserMode::Local => {
                let scope = button(
                    row![
                        text("SCOPE").size(9).color(th::text_muted()),
                        text(self.state.browser.search_scope_label())
                            .size(9)
                            .color(th::text())
                    ]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::Browser(BrowserMsg::CycleSearchScope))
                .padding([3, 0])
                .style(browser_utility_action_style);
                let location = self
                    .state
                    .browser
                    .current_folder
                    .as_ref()
                    .and_then(|folder| folder.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "All Roots".into());
                row![
                    scope,
                    horizontal_space(),
                    text(location)
                        .size(9)
                        .color(th::text_dim())
                        .wrapping(iced::widget::text::Wrapping::None)
                ]
                .align_y(iced::Alignment::Center)
                .into()
            }
            SampleBrowserMode::Remote => {
                let scope_label = match self.state.browser.search_scope {
                    crate::state::BrowserSearchScope::SelectedFolder => "THIS FOLDER",
                    crate::state::BrowserSearchScope::Root => "THIS CONNECTION",
                    crate::state::BrowserSearchScope::Everywhere => "EVERYWHERE",
                };
                let location = self
                    .state
                    .browser
                    .remote
                    .current_path
                    .rsplit('/')
                    .find(|part| !part.is_empty())
                    .unwrap_or("Alex's Dropbox");
                row![
                    button(
                        row![
                            text("SCOPE").size(9).color(th::text_muted()),
                            text(scope_label).size(9).color(th::text())
                        ]
                        .spacing(5)
                    )
                    .on_press(Message::Browser(BrowserMsg::CycleSearchScope))
                    .padding([3, 0])
                    .style(browser_utility_action_style),
                    horizontal_space(),
                    text(location)
                        .size(9)
                        .color(th::text_dim())
                        .wrapping(iced::widget::text::Wrapping::None)
                ]
                .align_y(iced::Alignment::Center)
                .into()
            }
        };

        let body: Element<'_, Message> = match self.state.browser.mode {
            SampleBrowserMode::Local
                if !self.state.browser.search.trim().is_empty()
                    && self.state.browser.search_scope
                        == crate::state::BrowserSearchScope::Everywhere =>
            {
                self.view_remote_browser()
            }
            SampleBrowserMode::Local => self.view_local_sample_browser(),
            SampleBrowserMode::Remote => self.view_remote_browser(),
        };

        let content: Element<'_, Message> = row![
            container(self.view_browser_places())
                .width(Length::Fixed(places_width))
                .height(Length::Fill)
                .style(browser_places_style),
            body
        ]
        .height(Length::Fill)
        .into();

        container(
            column![
                container(column![title_row, search, search_context].spacing(6))
                    .padding([8, 10])
                    .style(browser_header_style),
                content,
                self.view_browser_audition_footer()
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
        super::views_shell::horizontal_pane_splitter(
            self.state.browser.dock_resize_active,
            Message::Browser(BrowserMsg::BeginDockResize),
        )
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

        let search_lower = self.state.browser.search.trim().to_lowercase();
        let searching = !search_lower.is_empty();
        let current_folder = self.state.browser.current_folder.as_deref();

        let mut root_results: Vec<&std::path::PathBuf> = if !searching && current_folder.is_none() {
            self.state.browser.roots.iter().collect()
        } else if searching && self.state.browser.search_scope_path().is_none() {
            self.state
                .browser
                .roots
                .iter()
                .filter(|root| {
                    root.display()
                        .to_string()
                        .to_lowercase()
                        .contains(&search_lower)
                })
                .collect()
        } else {
            Vec::new()
        };
        root_results.sort_by_key(|root| browser_root_name(root).to_lowercase());

        // Filtering and sorting the whole catalog is memoized on the
        // catalog revision + query + scope, so the per-frame cost here
        // is bounded by the 200-row window below, not the library size.
        let local_results = self.state.browser.local_results(&search_lower);

        let selected_source = self.state.browser.selected_source.as_ref();
        let wide_columns = self
            .state
            .browser
            .results_use_wide_columns(self.state.view.window_width);
        let table_header: Element<'_, Message> = if wide_columns {
            container(
                row![
                    text("NAME")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fill),
                    text("BPM")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fixed(36.0)),
                    text("LENGTH")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fixed(50.0)),
                    text("STATUS")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fixed(48.0))
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding {
                top: 5.0,
                right: 12.0,
                bottom: 5.0,
                left: 10.0,
            })
            .width(Length::Fill)
            .style(browser_table_header_style)
            .into()
        } else {
            container(
                row![
                    text("NAME")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fill),
                    text("STATUS")
                        .size(9)
                        .color(th::text_muted())
                        .width(Length::Fixed(42.0))
                ]
                .spacing(0)
                .align_y(iced::Alignment::Center),
            )
            .padding(iced::Padding {
                top: 5.0,
                right: 12.0,
                bottom: 5.0,
                left: 10.0,
            })
            .width(Length::Fill)
            .style(browser_table_header_style)
            .into()
        };
        let total_results =
            root_results.len() + local_results.folders.len() + local_results.entries.len();
        let visible_results = self.state.browser.visible_result_count(total_results);
        let mut remaining = visible_results;
        let mut entries_col = column![].spacing(1);

        let notice = self
            .state
            .browser
            .current_local_root()
            .and_then(|root| {
                self.state
                    .browser
                    .root_catalog_message(root)
                    .map(|message| (root, message))
            })
            .or_else(|| {
                self.state.browser.roots.iter().find_map(|root| {
                    self.state
                        .browser
                        .root_catalog_message(root)
                        .map(|message| (root, message))
                })
            });
        if let Some((root, message)) = &notice {
            entries_col = entries_col.push(
                container(
                    text(format!("{} · {message}", browser_root_name(root)))
                        .size(9)
                        .color(
                            if matches!(
                                self.state.browser.root_catalog_label(root),
                                "STALE" | "WATCH ERR" | "WARN"
                            ) {
                                th::danger()
                            } else {
                                th::text_dim()
                            },
                        )
                        .wrapping(iced::widget::text::Wrapping::None),
                )
                .padding([6, 8]),
            );
        }

        for root in root_results.into_iter().take(remaining) {
            entries_col = entries_col
                .push(self.view_local_folder_result(
                    browser_root_name(root),
                    format!("LOCAL ROOT · {}", root.display()),
                    self.state.browser.root_catalog_label(root).to_string(),
                    root.clone(),
                    wide_columns,
                ))
                .push(browser_row_divider());
            remaining = remaining.saturating_sub(1);
        }
        for &index in local_results.folders.iter().take(remaining) {
            let folder = &self.state.browser.folders[index];
            entries_col = entries_col
                .push(self.view_local_folder_result(
                    folder.name.clone(),
                    browser_folder_context(
                        &folder.root_path,
                        &folder.relative_path,
                        "FOLDER",
                        None,
                    ),
                    "FOLDER".into(),
                    folder.path.clone(),
                    wide_columns,
                ))
                .push(browser_row_divider());
            remaining = remaining.saturating_sub(1);
        }
        for &index in local_results.entries.iter().take(remaining) {
            let entry = &self.state.browser.entries[index];
            let selected = selected_source.is_some_and(|source| &entry.source == source);
            let cell_color = browser_result_cell_color(selected);
            let metadata_detail = browser_entry_metadata(entry);
            let bpm = if selected {
                self.state
                    .browser
                    .audition_bpm_confirmed
                    .or(self.state.browser.audition_bpm_suggestion)
                    .map(|bpm| format!("{bpm:.0}"))
                    .unwrap_or_else(|| "—".into())
            } else {
                "—".into()
            };
            let length = entry
                .duration_seconds
                .map(format_browser_duration)
                .unwrap_or_else(|| "—".into());
            let source_detail = browser_folder_context(
                &entry.root_path,
                &entry.relative_path,
                &metadata_detail,
                entry.file_size,
            );
            let compact_detail = format!("BPM {bpm} · {length} · {source_detail}");
            // mouse_area returns early if its child captures the event, so
            // iced Button underneath would swallow press events. Use a
            // plain container as the click target instead.
            let name_cell = column![
                text(entry.name.as_str())
                    .size(12)
                    .color(th::text())
                    .width(Length::Fill)
                    .height(Length::Fixed(14.0))
                    .wrapping(iced::widget::text::Wrapping::None),
                text(if wide_columns {
                    source_detail
                } else {
                    compact_detail
                })
                .size(9)
                .color(cell_color)
                .width(Length::Fill)
                .height(Length::Fixed(11.0))
                .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(2)
            .width(Length::Fill);
            let table_cells: Element<'_, Message> = if wide_columns {
                row![
                    name_cell,
                    text(bpm)
                        .size(10)
                        .color(cell_color)
                        .width(Length::Fixed(36.0)),
                    text(length)
                        .size(10)
                        .color(cell_color)
                        .width(Length::Fixed(50.0)),
                    text("LOCAL")
                        .size(9)
                        .color(cell_color)
                        .width(Length::Fixed(48.0))
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                row![
                    name_cell,
                    text("LOCAL")
                        .size(9)
                        .color(cell_color)
                        .width(Length::Fixed(42.0))
                ]
                .spacing(0)
                .align_y(iced::Alignment::Center)
                .into()
            };
            let entry_body = container(table_cells).padding([6, 8]).width(Length::Fill);
            let entry_dragger: Element<'_, Message> = mouse_area(entry_body)
                .on_press(Message::BeginPendingBrowserDrag(
                    entry.source.clone(),
                    entry.name.clone(),
                ))
                .on_release(Message::ClickLocalBrowserEntry(entry.source.clone()))
                .into();
            let selection_marker = container(text(""))
                .width(Length::Fixed(2.0))
                .height(Length::Fixed(43.0))
                .style(move |_theme: &Theme| container::Style {
                    background: selected.then(|| th::accent().into()),
                    ..Default::default()
                });
            let flat_row = container(
                row![selection_marker, entry_dragger]
                    .spacing(0)
                    .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: selected.then(|| th::accent_dim().into()),
                ..Default::default()
            });
            entries_col = entries_col.push(flat_row).push(browser_row_divider());
        }

        if total_results == 0 && notice.is_none() {
            entries_col = entries_col.push(
                container(
                    text(if searching {
                        "No media or folders match this scope"
                    } else {
                        "This location has no child folders or supported media"
                    })
                    .size(11)
                    .color(th::text_dim()),
                )
                .padding([8, 4]),
            );
        }

        if self.state.browser.has_more_results(total_results) {
            let hidden = total_results.saturating_sub(visible_results);
            entries_col = entries_col.push(browser_row_divider()).push(
                button(
                    text(format!(
                        "SHOW {} MORE",
                        hidden.min(crate::state::BROWSER_RESULTS_PAGE_SIZE)
                    ))
                    .size(9)
                    .color(th::text_dim()),
                )
                .on_press(Message::Browser(BrowserMsg::ShowMoreLocalResults))
                .padding([7, 8])
                .width(Length::Fill)
                .style(browser_utility_action_style),
            );
        }

        let count = if self.state.browser.scan_in_progress {
            format!("INDEXING… · {}", self.state.browser.entries.len())
        } else if self.state.browser.scan_error.is_some() {
            "STALE".into()
        } else if self.state.browser.scan_warnings.is_empty() {
            format!("{visible_results} / {total_results}")
        } else {
            format!(
                "{visible_results} / {total_results} · WARN {}",
                self.state.browser.scan_warnings.len()
            )
        };

        container(
            column![
                row![
                    text("RESULTS").size(9).color(th::text_muted()),
                    horizontal_space(),
                    text(count).size(9).color(th::text_dim())
                ]
                .align_y(iced::Alignment::Center),
                table_header,
                scrollable(
                    container(entries_col)
                        .width(Length::Fill)
                        .padding(iced::Padding {
                            right: 12.0,
                            ..Default::default()
                        })
                )
                .id(super::media::browser_results_scroll_id(
                    SampleBrowserMode::Local,
                ))
                .height(Length::Fill)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::default()
                ))
            ]
            .spacing(0)
            .padding(8)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(browser_results_style)
        .into()
    }

    fn view_local_folder_result(
        &self,
        name: String,
        context: String,
        status: String,
        destination: std::path::PathBuf,
        wide_columns: bool,
    ) -> Element<'_, Message> {
        let name_cell = column![
            text(format!("› {name}"))
                .size(12)
                .color(th::text())
                .width(Length::Fill)
                .height(Length::Fixed(14.0))
                .wrapping(iced::widget::text::Wrapping::None),
            text(context)
                .size(9)
                .color(th::text_dim())
                .width(Length::Fill)
                .height(Length::Fixed(11.0))
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(2)
        .width(Length::Fill);
        let cells: Element<'_, Message> = if wide_columns {
            row![
                name_cell,
                text("—")
                    .size(10)
                    .color(th::text_dim())
                    .width(Length::Fixed(36.0)),
                text("—")
                    .size(10)
                    .color(th::text_dim())
                    .width(Length::Fixed(50.0)),
                text(status)
                    .size(9)
                    .color(th::text_dim())
                    .width(Length::Fixed(48.0))
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            row![
                name_cell,
                text(status)
                    .size(9)
                    .color(th::text_dim())
                    .width(Length::Fixed(42.0))
            ]
            .align_y(iced::Alignment::Center)
            .into()
        };

        button(container(cells).padding([6, 8]).width(Length::Fill))
            .on_press(Message::Browser(BrowserMsg::SelectLocalFolder(Some(
                destination,
            ))))
            .padding(0)
            .width(Length::Fill)
            .style(browser_utility_action_style)
            .into()
    }
}
