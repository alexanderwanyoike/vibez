//! Remote sample-browser view rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{button, column, container, horizontal_space, mouse_area, row, scrollable, text};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::message::Message;
use crate::state::{SampleBrowserEntry, SampleBrowserMode};
use crate::theme as th;
use vibez_core::track::MediaSourceRef;

use super::views_browser_style::*;
use super::*;

impl App {
    pub(super) fn view_remote_browser(&self) -> Element<'_, Message> {
        let browser = &self.state.browser;
        let remote = &browser.remote;
        let query = browser.search.trim().to_ascii_lowercase();
        let searching = !query.is_empty();
        let current = remote.current_path.as_str();
        let in_current_tree = |entry: &crate::remote_provider::RemoteCatalogEntry| {
            current.is_empty()
                || entry.provider_item_id == current
                || entry
                    .provider_item_id
                    .strip_prefix(current)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        let mut results: Vec<&crate::remote_provider::RemoteCatalogEntry> = if searching {
            remote
                .catalog
                .entries
                .iter()
                .filter(|entry| entry.is_folder || entry.is_supported_audio())
                .filter(|entry| {
                    let in_scope = match browser.search_scope {
                        crate::state::BrowserSearchScope::SelectedFolder => in_current_tree(entry),
                        crate::state::BrowserSearchScope::Root
                        | crate::state::BrowserSearchScope::Everywhere => true,
                    };
                    in_scope
                        && (entry.name.to_ascii_lowercase().contains(&query)
                            || entry.path.to_ascii_lowercase().contains(&query))
                })
                .collect()
        } else {
            remote
                .catalog_child_indices(current)
                .iter()
                .filter_map(|&index| remote.catalog.entries.get(index))
                .filter(|entry| entry.is_folder || entry.is_supported_audio())
                .collect()
        };
        if searching {
            results.sort_by(|left, right| {
                (!left.is_folder, left.name.to_ascii_lowercase())
                    .cmp(&(!right.is_folder, right.name.to_ascii_lowercase()))
            });
        }
        let mut local_everywhere: Vec<&SampleBrowserEntry> =
            if searching && browser.search_scope == crate::state::BrowserSearchScope::Everywhere {
                browser
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.name.to_ascii_lowercase().contains(&query)
                            || entry
                                .relative_path
                                .to_string_lossy()
                                .to_ascii_lowercase()
                                .contains(&query)
                    })
                    .collect()
            } else {
                Vec::new()
            };
        local_everywhere.sort_by_key(|entry| entry.name.to_ascii_lowercase());
        let total_results = results.len() + local_everywhere.len();
        let visible_results = total_results.min(browser.results_visible_limit);
        let remote_visible = results.len().min(visible_results);
        results.truncate(remote_visible);
        local_everywhere.truncate(visible_results.saturating_sub(remote_visible));
        let wide_columns = browser.results_use_wide_columns(self.state.view.window_width);
        let catalog_label = if remote.catalog_state == crate::state::RemoteCatalogState::Refreshing
        {
            if browser.effective_dock_width(self.state.view.window_width) < 360.0 {
                format!("REF {}P", remote.refresh_pages)
            } else {
                format!("REFRESHING {}P", remote.refresh_pages)
            }
        } else {
            remote.catalog_state.label().to_string()
        };
        let table_header: Element<'_, Message> = if wide_columns {
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
                text("AVAIL")
                    .size(9)
                    .color(th::text_muted())
                    .width(Length::Fixed(48.0))
            ]
            .spacing(4)
            .padding([5, 8])
            .into()
        } else {
            row![
                text("NAME")
                    .size(9)
                    .color(th::text_muted())
                    .width(Length::Fill),
                text("AVAIL")
                    .size(9)
                    .color(th::text_muted())
                    .width(Length::Fixed(42.0))
            ]
            .padding([5, 8])
            .into()
        };
        let mut entries_col = column![].spacing(0);
        let catalog_notice = match &remote.catalog_state {
            crate::state::RemoteCatalogState::Stale { error }
            | crate::state::RemoteCatalogState::AuthenticationRequired { error } => {
                Some(error.clone())
            }
            crate::state::RemoteCatalogState::Partial { pages, error } => {
                Some(format!("Partial refresh after {pages} page(s) · {error}"))
            }
            crate::state::RemoteCatalogState::Ready
            | crate::state::RemoteCatalogState::Refreshing => None,
        };
        if let Some(notice) = catalog_notice {
            entries_col = entries_col
                .push(container(text(notice).size(9).color(th::danger())).padding([5, 8]))
                .push(browser_row_divider());
        }
        for entry in results {
            let selected = remote.selected_path.as_deref() == Some(&entry.provider_item_id);
            let cell_color = if selected { th::text() } else { th::text_dim() };
            let mut context = if entry.parent_path.is_empty() {
                remote.catalog.connection_name.clone()
            } else {
                format!("{} · {}", remote.catalog.connection_name, entry.parent_path)
            };
            let metadata = entry.derived_metadata.as_ref().filter(|metadata| {
                metadata.provider_revision.as_deref() == entry.revision.as_deref()
            });
            if let Some(metadata) = metadata {
                let channels = match metadata.channels {
                    1 => "MONO".into(),
                    2 => "STEREO".into(),
                    channels => format!("{channels} CH"),
                };
                context = format!("{channels} · {} HZ · {context}", metadata.sample_rate);
            }
            // Availability is seeded from the persisted cache index and kept
            // current by fetch/import events; rendering must never stat the
            // filesystem per row.
            let availability = if entry.is_folder {
                crate::state::RemoteAvailability::RemoteOnly
            } else {
                match remote.availability.get(&entry.provider_item_id) {
                    Some(crate::state::RemoteAvailability::Fetching) => {
                        crate::state::RemoteAvailability::Fetching
                    }
                    Some(crate::state::RemoteAvailability::Unavailable { error }) => {
                        crate::state::RemoteAvailability::Unavailable {
                            error: error.clone(),
                        }
                    }
                    Some(crate::state::RemoteAvailability::Cached) => {
                        crate::state::RemoteAvailability::Cached
                    }
                    _ if remote.connected => crate::state::RemoteAvailability::RemoteOnly,
                    _ => crate::state::RemoteAvailability::ReconnectRequired,
                }
            };
            let availability_color = match &availability {
                crate::state::RemoteAvailability::Fetching => th::accent(),
                crate::state::RemoteAvailability::ReconnectRequired
                | crate::state::RemoteAvailability::Unavailable { .. } => th::danger(),
                crate::state::RemoteAvailability::RemoteOnly
                | crate::state::RemoteAvailability::Cached => cell_color,
            };
            if matches!(
                availability,
                crate::state::RemoteAvailability::ReconnectRequired
            ) {
                context = format!("RECONNECT REQUIRED · {context}");
            }
            let bpm = metadata
                .and_then(|metadata| metadata.bpm)
                .map(|bpm| format!("{bpm:.0}"))
                .unwrap_or_else(|| "—".into());
            let length = metadata
                .map(|metadata| format_browser_duration(metadata.duration_seconds))
                .unwrap_or_else(|| "—".into());
            if !wide_columns && !entry.is_folder {
                context = format!("BPM {bpm} · {length} · {context}");
            }
            let name_cell = column![
                text(if entry.is_folder {
                    format!("› {}", entry.name)
                } else {
                    entry.name.clone()
                })
                .size(12)
                .color(th::text())
                .width(Length::Fill)
                .height(Length::Fixed(14.0))
                .wrapping(iced::widget::text::Wrapping::None),
                text(context)
                    .size(9)
                    .color(cell_color)
                    .width(Length::Fill)
                    .height(Length::Fixed(11.0))
                    .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(2)
            .width(Length::Fill);
            let cells: Element<'_, Message> = if wide_columns {
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
                    text(availability.label())
                        .size(9)
                        .color(availability_color)
                        .width(Length::Fixed(48.0))
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                row![
                    name_cell,
                    text(availability.label())
                        .size(9)
                        .color(availability_color)
                        .width(Length::Fixed(42.0))
                ]
                .align_y(iced::Alignment::Center)
                .into()
            };
            let message = if entry.is_folder {
                Message::Browser(BrowserMsg::SelectRemoteFolder(
                    entry.provider_item_id.clone(),
                ))
            } else {
                Message::ClickRemoteBrowserEntry((*entry).clone())
            };
            let body = container(cells).padding([6, 8]).width(Length::Fill);
            let interactive: Element<'_, Message> = if entry.is_folder {
                button(body)
                    .on_press(message)
                    .padding(0)
                    .width(Length::Fill)
                    .style(browser_utility_action_style)
                    .into()
            } else {
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.provider_item_id.clone(),
                    display_path: entry.path.clone(),
                    rev: entry.revision.clone(),
                };
                mouse_area(body)
                    .on_press(Message::BeginPendingBrowserDrag(source, entry.name.clone()))
                    .on_release(message)
                    .into()
            };
            let selection_marker = container(text(""))
                .width(Length::Fixed(2.0))
                .height(Length::Fixed(43.0))
                .style(move |_theme: &Theme| container::Style {
                    background: selected.then(|| th::accent().into()),
                    ..Default::default()
                });
            entries_col = entries_col
                .push(
                    container(row![selection_marker, interactive])
                        .width(Length::Fill)
                        .style(move |_theme: &Theme| container::Style {
                            background: selected.then(|| th::accent_dim().into()),
                            ..Default::default()
                        }),
                )
                .push(browser_row_divider());
        }
        for entry in local_everywhere {
            let selected = browser.selected_source.as_ref() == Some(&entry.source);
            let cell_color = if selected { th::text() } else { th::text_dim() };
            let name_cell = column![
                text(entry.name.as_str())
                    .size(12)
                    .color(th::text())
                    .width(Length::Fill)
                    .height(Length::Fixed(14.0))
                    .wrapping(iced::widget::text::Wrapping::None),
                text(format!(
                    "Local · {}/{}",
                    browser_root_name(&entry.root_path),
                    entry.relative_path.display()
                ))
                .size(9)
                .color(cell_color)
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
                        .color(cell_color)
                        .width(Length::Fixed(36.0)),
                    text(
                        entry
                            .duration_seconds
                            .map(format_browser_duration)
                            .unwrap_or_else(|| "—".into())
                    )
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
                .align_y(iced::Alignment::Center)
                .into()
            };
            let body = container(cells).padding([6, 8]).width(Length::Fill);
            let interactive: Element<'_, Message> = mouse_area(body)
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
            entries_col = entries_col
                .push(
                    container(row![selection_marker, interactive])
                        .width(Length::Fill)
                        .style(move |_theme: &Theme| container::Style {
                            background: selected.then(|| th::accent_dim().into()),
                            ..Default::default()
                        }),
                )
                .push(browser_row_divider());
        }
        if total_results == 0 {
            let empty = if remote.catalog.entries.is_empty() {
                "No saved Remote metadata yet; connect and refresh when available"
            } else if searching {
                "No media or folders match this scope"
            } else {
                "This Remote folder has no child folders or supported media"
            };
            entries_col = entries_col
                .push(container(text(empty).size(11).color(th::text_dim())).padding([8, 4]));
        }
        if browser.has_more_results(total_results) {
            entries_col = entries_col.push(
                button(text("SHOW MORE").size(9).color(th::text_dim()))
                    .on_press(Message::Browser(BrowserMsg::ShowMoreLocalResults))
                    .padding([7, 8])
                    .width(Length::Fill)
                    .style(browser_utility_action_style),
            );
        }
        let refresh = button(text("REF").size(9).color(th::text_dim()))
            .on_press(Message::RefreshRemoteConnection)
            .padding([3, 5])
            .style(browser_utility_action_style);
        let state_color = if matches!(
            remote.catalog_state,
            crate::state::RemoteCatalogState::Stale { .. }
                | crate::state::RemoteCatalogState::Partial { .. }
                | crate::state::RemoteCatalogState::AuthenticationRequired { .. }
        ) {
            th::danger()
        } else {
            th::text_dim()
        };

        container(
            column![
                row![
                    text("RESULTS").size(9).color(th::text_muted()),
                    horizontal_space(),
                    text(format!(
                        "{} · {visible_results}/{total_results}",
                        catalog_label
                    ))
                    .size(9)
                    .color(state_color),
                    refresh
                ]
                .spacing(5)
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
                    SampleBrowserMode::Remote,
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
}
