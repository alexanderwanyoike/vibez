//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::Path;

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, row, scrollable, slider, text,
    text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;

use crate::icons;
use crate::message::Message;
use crate::state::{AuditionMode, SampleBrowserEntry, SampleBrowserMode};
use crate::theme as th;
use vibez_engine::commands::AuditionSync;

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
            SampleBrowserMode::Dropbox => row![
                text("SCOPE").size(9).color(th::text_muted()),
                text("REMOTE CATALOG").size(9).color(th::text_dim())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center)
            .into(),
        };

        let body: Element<'_, Message> = match self.state.browser.mode {
            SampleBrowserMode::Local => self.view_local_sample_browser(),
            SampleBrowserMode::Dropbox => self.view_dropbox_browser(),
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
                    "Disconnected"
                })
                .size(10)
                .color(connection_color),
            )
            .padding([3, 0]),
        ]);

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

    fn view_browser_audition_footer(&self) -> Element<'_, Message> {
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
        let enabled = self.state.browser.audition_enabled;
        let follow_toggle = button(
            text(if enabled { "ENABLED ON" } else { "ENABLED OFF" })
                .size(9)
                .color(if enabled {
                    th::accent()
                } else {
                    th::text_dim()
                }),
        )
        .on_press(Message::ToggleAuditionEnabled)
        .padding([2, 4])
        .style(browser_utility_action_style);
        let import_label = match self.state.browser.audition_import_input() {
            Some(input) if input.mode == AuditionMode::Raw => "IMPORT RAW".to_string(),
            Some(input) => format!("IMPORT WARP {:.1}", input.source_bpm.unwrap_or_default()),
            None => "IMPORT BLOCKED".to_string(),
        };

        let raw_active = self.state.browser.audition_mode == AuditionMode::Raw;
        let raw = button(text("RAW").size(9))
            .on_press(Message::SetAuditionMode(AuditionMode::Raw))
            .padding([2, 5])
            .style(move |_theme: &Theme, status| browser_place_button_style(raw_active, status));
        let warp_active = self.state.browser.audition_mode == AuditionMode::Warp;
        let warp = button(text("WARP").size(9))
            .on_press(Message::SetAuditionMode(AuditionMode::Warp))
            .padding([2, 5])
            .style(move |_theme: &Theme, status| browser_place_button_style(warp_active, status));
        let sync_button = |label, value| {
            let active = self.state.browser.audition_sync == value;
            button(text(label).size(9))
                .on_press(Message::SetAuditionSync(value))
                .padding([2, 4])
                .style(move |_theme: &Theme, status| browser_place_button_style(active, status))
        };
        let looped = self.state.browser.audition_loop;
        let loop_toggle = button(text(if looped { "LOOP ON" } else { "LOOP OFF" }).size(9))
            .on_press(Message::ToggleAuditionLoop)
            .padding([2, 4])
            .style(move |_theme: &Theme, status| browser_place_button_style(looped, status));

        let bpm_input = text_input("BPM", &self.state.browser.audition_bpm_edit)
            .on_input(Message::AuditionBpmEditChanged)
            .on_submit(Message::ConfirmAuditionBpm)
            .size(10)
            .padding([3, 5])
            .width(Length::Fixed(62.0))
            .style(browser_compact_input_style);
        let confirm_bpm = button(text("CONFIRM").size(9))
            .on_press(Message::ConfirmAuditionBpm)
            .padding([3, 5])
            .style(browser_utility_action_style);
        let bpm_state = if self.state.browser.audition_bpm_detecting {
            "DETECTING".to_string()
        } else if let Some(confirmed) = self.state.browser.audition_bpm_confirmed {
            format!("CONFIRMED {confirmed:.1}")
        } else if let Some(suggestion) = self.state.browser.audition_bpm_suggestion {
            let low = self
                .state
                .browser
                .audition_bpm_confidence
                .is_some_and(|confidence| confidence < self.state.warp_confidence_threshold);
            if low {
                format!("SUGGEST {suggestion:.1} · LOW")
            } else {
                format!("SUGGEST {suggestion:.1}")
            }
        } else {
            "MANUAL FOR WARP".to_string()
        };

        let waveform: Element<'_, Message> = container(
            canvas(crate::widgets::browser_waveform::BrowserWaveform {
                audio: self.state.browser.waveform_audio.clone(),
            })
            .width(Length::Fill)
            .height(Length::Fixed(26.0)),
        )
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            border: iced::Border {
                color: th::divider(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into();

        let controls = row![
            play,
            stop,
            text(if self.state.browser.dropbox.preview_in_progress {
                "FETCHING"
            } else if self.state.browser.audition_loading {
                "PREPARING"
            } else if self.state.browser.audition_queued {
                "QUEUED"
            } else if self.state.browser.audition_playing {
                "PLAYING"
            } else if self.state.browser.waveform_error.is_some() {
                "UNAVAILABLE"
            } else if self.state.browser.waveform_audio.is_some() {
                match self.state.browser.audition_mode {
                    AuditionMode::Raw => "RAW",
                    AuditionMode::Warp if self.state.browser.audition_bpm_confirmed.is_some() => {
                        "WARP READY"
                    }
                    AuditionMode::Warp => "BPM NEEDED",
                }
            } else {
                "SELECT"
            })
            .size(9)
            .color(th::text_dim()),
            waveform
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center);
        let gain = self.state.browser.audition_gain;
        let gain_slider = slider(0.0..=2.0, gain, Message::SetAuditionGain)
            .step(0.01)
            .width(Length::Fill)
            .style(|_theme: &Theme, status| iced::widget::slider::Style {
                rail: iced::widget::slider::Rail {
                    backgrounds: (th::accent_dim().into(), th::divider().into()),
                    width: 2.0,
                    border: iced::Border::default(),
                },
                handle: iced::widget::slider::Handle {
                    shape: iced::widget::slider::HandleShape::Rectangle {
                        width: 6,
                        border_radius: 0.0.into(),
                    },
                    background: if matches!(status, iced::widget::slider::Status::Dragged) {
                        th::accent().into()
                    } else {
                        th::text_dim().into()
                    },
                    border_width: 0.0,
                    border_color: iced::Color::TRANSPARENT,
                },
            });
        let gain_row = row![
            text("GAIN").size(9).color(th::text_muted()),
            gain_slider,
            text(audition_gain_label(gain))
                .size(9)
                .color(th::text_dim())
                .width(Length::Fixed(48.0))
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let contents: Element<'_, Message> = column![
            row![
                text("AUDITION").size(9).color(th::text_muted()),
                follow_toggle,
                text(import_label).size(9).color(th::text_dim()),
                text(selected_label)
                    .size(10)
                    .color(th::text_dim())
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
            row![
                text("MODE").size(9).color(th::text_muted()),
                raw,
                warp,
                text("SYNC").size(9).color(th::text_muted()),
                sync_button("OFF", AuditionSync::Off),
                sync_button("BEAT", AuditionSync::Beat),
                sync_button("BAR", AuditionSync::Bar),
                horizontal_space(),
                loop_toggle,
            ]
            .spacing(3)
            .align_y(iced::Alignment::Center),
            row![
                text("BPM").size(9).color(th::text_muted()),
                bpm_input,
                confirm_bpm,
                text(bpm_state)
                    .size(9)
                    .color(if self.state.browser.audition_bpm_confirmed.is_some() {
                        th::accent()
                    } else {
                        th::text_dim()
                    })
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
            controls,
            gain_row
        ]
        .spacing(5)
        .into();

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

        let mut folder_results: Vec<_> = self
            .state
            .browser
            .folders
            .iter()
            .filter(|folder| {
                self.state
                    .browser
                    .local_folder_is_result(folder, &search_lower)
            })
            .collect();
        folder_results.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.path.cmp(&b.path))
        });

        let mut filtered_entries: Vec<&SampleBrowserEntry> = self
            .state
            .browser
            .entries
            .iter()
            .filter(|entry| {
                self.state
                    .browser
                    .local_entry_is_result(entry, &search_lower)
            })
            .collect();
        filtered_entries.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });

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
        let total_results = root_results.len() + folder_results.len() + filtered_entries.len();
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
        for folder in folder_results.into_iter().take(remaining) {
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
        for entry in filtered_entries.into_iter().take(remaining) {
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
                text(browser_folder_context(
                    &entry.root_path,
                    &entry.relative_path,
                    &metadata_detail,
                    entry.file_size,
                ))
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
                scrollable(
                    container(entries_col)
                        .width(Length::Fill)
                        .padding(iced::Padding {
                            right: 12.0,
                            ..Default::default()
                        })
                )
                .height(Length::Fill)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::default()
                ))
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
                    .width(Length::Fill);
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower.clone(),
                    display_path: entry.path_display.clone(),
                    rev: entry.rev.clone(),
                };
                let dragger: Element<'_, Message> = mouse_area(row_body)
                    .on_press(Message::BeginPendingBrowserDrag(source, entry.name.clone()))
                    .on_release(msg)
                    .into();
                let selection_marker = container(text(""))
                    .width(Length::Fixed(2.0))
                    .height(Length::Fixed(25.0))
                    .style(move |_theme: &Theme| container::Style {
                        background: selected.then(|| th::accent().into()),
                        ..Default::default()
                    });
                rows.push(
                    container(
                        row![selection_marker, dragger]
                            .spacing(2)
                            .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fill)
                    .style(move |_theme: &Theme| container::Style {
                        background: selected.then(|| th::accent_dim().into()),
                        ..Default::default()
                    })
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
                            _ => None,
                        }
                    };
                    button::Style {
                        background: bg,
                        text_color: if selected { th::accent() } else { th::text() },
                        border: iced::Border {
                            radius: 0.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });
                rows.push(btn.into());
            }

            rows.push(browser_row_divider());

            if entry.is_folder && expanded {
                self.render_dropbox_tree(entry.path_lower.clone(), depth + 1, rows);
            }
        }
    }
}

fn browser_root_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

fn browser_folder_context(
    root: &Path,
    relative_path: &Path,
    detail: &str,
    file_size: Option<u64>,
) -> String {
    let size = file_size
        .map(format_browser_file_size)
        .map(|size| format!(" · {size}"))
        .unwrap_or_default();
    format!(
        "{detail}{size} · {}/{}",
        browser_root_name(root),
        relative_path.display()
    )
}

fn browser_entry_metadata(entry: &SampleBrowserEntry) -> String {
    let channels = entry.channels.map(|channels| match channels {
        1 => "MONO".into(),
        2 => "STEREO".into(),
        channels => format!("{channels} CH"),
    });
    let sample_rate = entry.sample_rate.map(|sample_rate| {
        if sample_rate % 1_000 == 0 {
            format!("{} KHZ", sample_rate / 1_000)
        } else {
            format!("{:.1} KHZ", sample_rate as f64 / 1_000.0)
        }
    });
    std::iter::once(entry.format.clone())
        .chain(channels)
        .chain(sample_rate)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_browser_duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        let total_seconds = seconds.round() as u64;
        format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
    } else {
        format!("{seconds:.1}s")
    }
}

fn format_browser_file_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if (bytes as f64) < MIB {
        format!("{:.1} KB", bytes as f64 / KIB)
    } else {
        format!("{:.1} MB", bytes as f64 / MIB)
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
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_row_divider<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

fn audition_gain_label(gain: f32) -> String {
    if gain <= 0.0001 {
        "−∞ dB".into()
    } else {
        format!("{:+.1} dB", 20.0 * gain.log10())
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
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_utility_action_style(_theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| th::bg_hover().into()),
        text_color: if matches!(status, button::Status::Pressed) {
            th::accent()
        } else {
            th::text_dim()
        },
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn browser_compact_input_style(
    _theme: &Theme,
    _status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
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
            radius: 0.0.into(),
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

fn browser_table_header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_dark().into()),
        ..Default::default()
    }
}

fn browser_result_cell_color(selected: bool) -> iced::Color {
    if selected {
        th::text()
    } else {
        th::text_dim()
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

#[cfg(test)]
mod browser_table_tests {
    use super::*;

    #[test]
    fn selected_result_metadata_uses_the_selected_foreground() {
        assert_eq!(browser_result_cell_color(true), th::text());
        assert_eq!(browser_result_cell_color(false), th::text_dim());
    }

    #[test]
    fn decoded_metadata_is_compact_and_truthful() {
        let entry = SampleBrowserEntry {
            source: MediaSourceRef::LocalFile {
                path: "/samples/loop.aiff".into(),
            },
            name: "loop.aiff".into(),
            root_path: "/samples".into(),
            relative_path: "loop.aiff".into(),
            format: "AIFF".into(),
            duration_seconds: Some(119.6),
            channels: Some(2),
            sample_rate: Some(48_000),
            file_size: Some(42),
            modified: None,
            search_text: "loop aiff".into(),
        };
        assert_eq!(browser_entry_metadata(&entry), "AIFF · STEREO · 48 KHZ");
        assert_eq!(
            format_browser_duration(entry.duration_seconds.unwrap()),
            "2:00"
        );
    }
}
