//! Modal overlays: context menus, rename prompt, track deletion. The File
//! menu lives in views_file_menu.rs.

//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, scrollable, text,
    text_input, vertical_space,
};
use iced::{Element, Length, Theme};

use crate::domains::arrangement::{ArrangementMsg, AudioSliceMarkers};
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::effect::EffectType;
use vibez_core::midi::InstrumentKind;
use vibez_plugin_host::{PluginCategory, PluginFormat};

use crate::icons;
use crate::message::{MenuOverlay, Message};
use crate::state::ContextMenuTarget;
use crate::theme as th;

use super::*;

fn project_track_deletion_list_height(location_count: usize) -> f32 {
    match location_count {
        0 | 1 => 28.0,
        count => (count as f32 * 28.0 + (count - 1) as f32 * 4.0).min(120.0),
    }
}

const CONTEXT_MENU_EDGE_INSET: f32 = 16.0;
const CONTEXT_MENU_CARD_PADDING: f32 = 8.0;
const CONTEXT_MENU_CLIP_WIDTH: f32 = 220.0 + CONTEXT_MENU_CARD_PADDING;

fn context_menu_width(target: &ContextMenuTarget) -> f32 {
    match target {
        ContextMenuTarget::Clip { .. } => CONTEXT_MENU_CLIP_WIDTH,
        ContextMenuTarget::TimeSelection { .. }
        | ContextMenuTarget::AudioClipDetail { .. }
        | ContextMenuTarget::ArrangementEmpty => 200.0 + CONTEXT_MENU_CARD_PADDING,
    }
}

fn context_menu_x(requested_x: f32, menu_width: f32, window_width: f32) -> f32 {
    let maximum_x = (window_width - menu_width - CONTEXT_MENU_EDGE_INSET).max(0.0);
    requested_x.clamp(0.0, maximum_x)
}

fn drum_rack_slice_choice(
    label: &'static str,
    count: usize,
    selected: bool,
    markers: AudioSliceMarkers,
) -> Element<'static, Message> {
    button(
        column![
            text(label)
                .size(13)
                .color(if selected { th::accent() } else { th::text() }),
            text(if count == 0 {
                "No interior slices".into()
            } else {
                format!("{count} slices")
            })
            .size(10)
            .color(th::text_dim())
        ]
        .spacing(3),
    )
    .on_press(Message::SetDrumRackSliceMarkers(markers))
    .padding([9, 11])
    .width(Length::Fill)
    .style(move |_theme: &Theme, status| button::Style {
        background: Some(
            if selected {
                th::accent_dim()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: if selected { th::accent() } else { th::border() },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn drum_rack_slice_can_create(slice_count: usize) -> bool {
    slice_count > 0 && slice_count <= vibez_core::track::DRUM_RACK_PAD_COUNT
}

impl App {
    pub(super) fn view_drum_rack_slice_overlay(&self) -> Element<'_, Message> {
        let dialog = self
            .state
            .view
            .drum_rack_slice_dialog
            .expect("Drum Rack slice overlay requires a pending request");
        let clip = self
            .timeline_content_at(dialog.location, dialog.track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == dialog.clip_id));
        let transient_count = clip.map_or(0, |clip| {
            crate::domains::arrangement::slice_region_count(clip, AudioSliceMarkers::Transients)
        });
        let warp_count = clip.map_or(0, |clip| {
            crate::domains::arrangement::slice_region_count(clip, AudioSliceMarkers::Warp)
        });
        let selected_count = match dialog.markers {
            AudioSliceMarkers::Transients => transient_count,
            AudioSliceMarkers::Warp => warp_count,
        };
        let pad_count = vibez_core::track::DRUM_RACK_PAD_COUNT;
        let can_create = drum_rack_slice_can_create(selected_count);
        let guidance = if selected_count == 0 {
            "This marker type has no interior slices.".to_string()
        } else if selected_count > pad_count {
            format!(
                "{selected_count} slices cannot fit {pad_count} pads. Use fewer markers or choose Warp markers."
            )
        } else {
            format!("{selected_count} slices will fill {selected_count} of {pad_count} pads.")
        };
        let cancel = button(text("Cancel").size(12).color(th::text()))
            .on_press(Message::CancelDrumRackSlice)
            .padding([7, 14]);
        let create = button(text("Create Drum Rack").size(12).color(th::bg_dark()))
            .on_press_maybe(can_create.then_some(Message::ConfirmDrumRackSlice))
            .padding([7, 14])
            .style(move |_theme: &Theme, status| {
                let background = if !can_create {
                    th::text_muted()
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::accent_dim()
                } else {
                    th::accent()
                };
                button::Style {
                    background: Some(background.into()),
                    text_color: th::bg_dark(),
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });
        let card = container(
            column![
                text("Slice to Drum Rack").size(18).color(th::text()),
                text("Choose which timing markers become pads.")
                    .size(11)
                    .color(th::text_dim()),
                row![
                    drum_rack_slice_choice(
                        "Transient markers",
                        transient_count,
                        dialog.markers == AudioSliceMarkers::Transients,
                        AudioSliceMarkers::Transients,
                    ),
                    drum_rack_slice_choice(
                        "Warp markers",
                        warp_count,
                        dialog.markers == AudioSliceMarkers::Warp,
                        AudioSliceMarkers::Warp,
                    ),
                ]
                .spacing(8),
                text(guidance).size(11).color(if can_create {
                    th::text_dim()
                } else {
                    th::danger()
                }),
                text("A MIDI clip will be created to reconstruct the original rhythm.")
                    .size(10)
                    .color(th::text_muted()),
                row![horizontal_space(), cancel, create]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
            ]
            .spacing(12),
        )
        .width(Length::Fixed(440.0))
        .padding(18)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        mouse_area(
            center(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelDrumRackSlice)
        .into()
    }

    pub(super) fn view_track_deletion_overlay(&self) -> Element<'_, Message> {
        let track_id = self
            .state
            .arrangement
            .pending_project_track_deletion
            .expect("track deletion overlay requires a pending track");
        let track_name = self
            .state
            .project_tracks
            .find(track_id)
            .map(|track| track.name.as_str())
            .unwrap_or("Missing Project Track");
        let locations = self
            .state
            .perform
            .sections
            .track_content_locations(&self.state.arrangement.timeline, track_id);
        let location_list_height = project_track_deletion_list_height(locations.len());
        let mut location_list = column![].spacing(4);
        if locations.is_empty() {
            location_list = location_list.push(
                text("No authored timeline content")
                    .size(11)
                    .color(th::text_dim()),
            );
        } else {
            for location in locations {
                let label = match location {
                    crate::domains::perform::TimelineContentLocation::Arrange => "Arrange".into(),
                    crate::domains::perform::TimelineContentLocation::Section { slot, name } => {
                        format!("Section {:02} · {name}", slot + 1)
                    }
                };
                location_list = location_list.push(
                    container(
                        row![
                            container(horizontal_space())
                                .width(Length::Fixed(3.0))
                                .height(Length::Fixed(14.0))
                                .style(|_theme: &Theme| container::Style {
                                    background: Some(th::danger().into()),
                                    ..Default::default()
                                }),
                            text(label).size(11).color(th::text())
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([5, 8])
                    .width(Length::Fill)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::bg_elevated().into()),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }
        }
        let cancel = button(text("Cancel").size(12).color(th::text()))
            .on_press(Message::Arrangement(
                crate::domains::arrangement::ArrangementMsg::CancelRemoveTrack,
            ))
            .padding([7, 14]);
        let remove = button(text("Delete Project Track").size(12).color(th::bg_dark()))
            .on_press(Message::Arrangement(
                crate::domains::arrangement::ArrangementMsg::ConfirmRemoveTrack(track_id),
            ))
            .padding([7, 14])
            .style(|_theme: &Theme, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        th::blend(th::danger(), th::text(), 0.18)
                    } else {
                        th::danger()
                    }
                    .into(),
                ),
                text_color: th::bg_dark(),
                border: iced::Border::default(),
                ..Default::default()
            });
        let card = container(
            column![
                text(format!("Delete {track_name}?"))
                    .font(crate::typography::PERFORM_DISPLAY)
                    .size(18)
                    .color(th::text()),
                text("This Project Track is shared. Its channel, devices, and authored content will be removed from:")
                    .size(11)
                    .color(th::text_dim()),
                scrollable(location_list).height(Length::Fixed(location_list_height)),
                text("One Undo restores the Track and every listed location.")
                    .size(10)
                    .color(th::text_dim()),
                row![horizontal_space(), cancel, remove].spacing(8),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fixed(440.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });
        container(center(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_edit_menu_overlay(&self) -> Element<'_, Message> {
        let item = |icon: char, label: &'static str, shortcut: &'static str, message: Message| {
            button(
                row![
                    icons::icon(icon).size(12).color(th::text()),
                    text(label).size(12).color(th::text()),
                    horizontal_space(),
                    text(shortcut).size(10).color(th::text_dim()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::menu_item(MenuOverlay::Edit, message))
            .padding([7, 12])
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
            })
        };

        let menu = column![
            item(
                icons::COPY,
                "Copy",
                "Ctrl+C",
                Message::Arrangement(ArrangementMsg::CopySelectedClips),
            ),
            item(
                icons::SCISSORS,
                "Cut",
                "Ctrl+X",
                Message::Arrangement(ArrangementMsg::CutSelectedClips),
            ),
            item(
                icons::COPY,
                "Paste",
                "Ctrl+V",
                Message::Arrangement(ArrangementMsg::PasteClips),
            ),
            item(
                icons::COPY,
                "Duplicate",
                "",
                Message::Arrangement(ArrangementMsg::DuplicateSelectedClip),
            ),
            item(
                icons::REPEAT,
                "Toggle Clip Loop",
                "Ctrl+Shift+L",
                Message::Arrangement(ArrangementMsg::ToggleSelectedClipLoop),
            ),
            item(
                icons::SCISSORS,
                "Split Selection",
                "Ctrl+E",
                Message::split_selected_at_playhead(),
            ),
            item(
                icons::COPY,
                "Join Clips",
                "Ctrl+J",
                Message::join_selected_clips(),
            ),
            item(
                icons::SCISSORS,
                "Trim Track Mutes",
                "",
                Message::Arrangement(ArrangementMsg::TrimSelectedByTrackMutes),
            ),
        ]
        .spacing(1)
        .padding(4)
        .width(Length::Fixed(260.0));

        let card = container(menu).style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });
        let positioned = column![
            vertical_space().height(Length::Fixed(42.0)),
            row![horizontal_space().width(Length::Fixed(112.0)), card]
        ];
        mouse_area(
            container(positioned)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::dismiss_menu(MenuOverlay::Edit))
        .into()
    }

    pub(super) fn view_device_context_menu_overlay(&self) -> Element<'_, Message> {
        use crate::state::DeviceMenuCategory;

        let menu = self.state.devices.context_menu.as_ref().unwrap();
        let track_id = menu.track_id;
        let is_midi = self
            .state
            .find_track(track_id)
            .is_some_and(|t| t.kind.is_midi());

        // Category tabs
        let mut tabs_row = row![].spacing(2);
        if is_midi {
            let inst_active = menu.category == Some(DeviceMenuCategory::Instruments);
            let (bg, tc) = if inst_active {
                (th::accent_dim(), th::accent())
            } else {
                (th::bg_elevated(), th::text_dim())
            };
            let inst_tab = button(text("Instruments").size(11).color(tc))
                .on_press(Message::set_device_menu_category(
                    DeviceMenuCategory::Instruments,
                ))
                .padding([4, 10])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if inst_active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            tabs_row = tabs_row.push(inst_tab);
        }
        let fx_active = menu.category == Some(DeviceMenuCategory::Effects);
        let (bg, tc) = if fx_active {
            (th::accent_dim(), th::accent())
        } else {
            (th::bg_elevated(), th::text_dim())
        };
        let fx_tab = button(text("Effects").size(11).color(tc))
            .on_press(Message::set_device_menu_category(
                DeviceMenuCategory::Effects,
            ))
            .padding([4, 10])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color: tc,
                border: iced::Border {
                    color: if fx_active {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
        tabs_row = tabs_row.push(fx_tab);

        // Plugins tab
        let plugins_active = menu.category == Some(DeviceMenuCategory::Plugins);
        let (bg, tc) = if plugins_active {
            (th::accent_dim(), th::accent())
        } else {
            (th::bg_elevated(), th::text_dim())
        };
        let plugins_tab = button(text("Plugins").size(11).color(tc))
            .on_press(Message::set_device_menu_category(
                DeviceMenuCategory::Plugins,
            ))
            .padding([4, 10])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color: tc,
                border: iced::Border {
                    color: if plugins_active {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
        tabs_row = tabs_row.push(plugins_tab);

        // Search input
        let search_input = text_input("Search...", &menu.search)
            .on_input(Message::device_menu_search)
            .size(12)
            .width(Length::Fill);

        // Items list
        const PLUGIN_GRID_COLS: usize = 4;
        const PLUGIN_GRID_COL_W: f32 = 150.0;
        let mut items_col = column![].spacing(2);
        let search_lower = menu.search.to_lowercase();
        // Estimated visible rows, used to size and clamp the popup.
        let mut est_rows: usize = 0;
        let mut is_grid = false;

        match menu.category {
            Some(DeviceMenuCategory::Instruments) => {
                for &kind in InstrumentKind::all() {
                    let name = kind.name();
                    if !search_lower.is_empty() && !name.to_lowercase().contains(&search_lower) {
                        continue;
                    }
                    let btn = button(text(name).size(12).color(th::text()))
                        .on_press(Message::set_track_instrument(track_id, kind))
                        .padding([6, 10])
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
                        });
                    items_col = items_col.push(btn);
                    est_rows += 1;
                }
            }
            Some(DeviceMenuCategory::Plugins) => {
                is_grid = true;
                if self.state.plugin_settings.cache.is_empty() {
                    items_col = items_col.push(
                        text("No plugins scanned yet.\nUse File → Settings to scan.")
                            .size(11)
                            .color(th::text_dim()),
                    );
                    est_rows = 2;
                } else {
                    let mut filtered: Vec<&vibez_plugin_host::PluginInfo> = self
                        .state
                        .plugin_settings
                        .cache
                        .iter()
                        .filter(|p| {
                            search_lower.is_empty()
                                || p.name.to_lowercase().contains(&search_lower)
                                || p.vendor.to_lowercase().contains(&search_lower)
                        })
                        .collect();
                    filtered.sort_by_key(|a| a.name.to_lowercase());
                    est_rows = filtered.len().div_ceil(PLUGIN_GRID_COLS);
                    for chunk in filtered.chunks(PLUGIN_GRID_COLS) {
                        let mut grid_row = row![].spacing(2);
                        for plugin in chunk {
                            let format_badge = match plugin.format {
                                PluginFormat::Clap => "CLAP",
                                PluginFormat::Vst3 => "VST3",
                            };
                            let cat_label = match plugin.category {
                                PluginCategory::Effect => "fx",
                                PluginCategory::Instrument => "inst",
                                PluginCategory::Both => "fx+inst",
                            };
                            let plugin_id = plugin.id.clone();
                            // Full name, wrapping inside the fixed
                            // cell width: truncated names made the
                            // LSP suite indistinguishable.
                            let cell = column![
                                text(plugin.name.clone()).size(11).color(th::text()),
                                text(format!("{format_badge} {cat_label}"))
                                    .size(9)
                                    .color(th::text_dim()),
                            ]
                            .spacing(1);
                            let btn = button(cell)
                                .on_press(Message::AddPluginToTrack(track_id, plugin_id))
                                .padding([4, 8])
                                .width(Length::Fixed(PLUGIN_GRID_COL_W))
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
                                });
                            grid_row = grid_row.push(btn);
                        }
                        items_col = items_col.push(grid_row);
                    }
                }
            }
            Some(DeviceMenuCategory::Effects) | None => {
                for &et in EffectType::all() {
                    let name = et.name();
                    if !search_lower.is_empty() && !name.to_lowercase().contains(&search_lower) {
                        continue;
                    }
                    let btn = button(text(name).size(12).color(th::text()))
                        .on_press(Message::add_effect(track_id, et))
                        .padding([6, 10])
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
                        });
                    items_col = items_col.push(btn);
                    est_rows += 1;
                }
            }
        }

        // Cap the list height and scroll it: a full plugin library is
        // hundreds of entries, which would otherwise render past the
        // bottom of the window and look like an empty menu. The
        // plugins tab uses a 4-column grid to spend the space on
        // breadth instead of one skinny endless column.
        const MENU_LIST_MAX_H: f32 = 380.0;
        let (menu_w, row_h) = if is_grid {
            (PLUGIN_GRID_COL_W * PLUGIN_GRID_COLS as f32 + 30.0, 38.0)
        } else {
            (220.0, 29.0)
        };
        let est_list_h = (est_rows.max(1) as f32 * row_h).min(MENU_LIST_MAX_H);
        let items_scroll = container(scrollable(items_col).width(Length::Fill).direction(
            scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ),
        ))
        .max_height(MENU_LIST_MAX_H);

        let menu_content = column![tabs_row, search_input, items_scroll]
            .spacing(6)
            .padding(8)
            .width(Length::Fixed(menu_w));

        let menu_card = container(menu_content).style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // Position the menu near where it was triggered, clamped just
        // enough that the estimated content stays on-screen (the
        // devices panel lives at the bottom of the window).
        let est_h = est_list_h + 90.0;
        let menu_y = menu.y.min(self.state.view.window_height - est_h).max(0.0);
        let menu_x = menu
            .x
            .min(self.state.view.window_width - menu_w - 16.0)
            .max(0.0);
        let padded = column![
            vertical_space().height(Length::Fixed(menu_y)),
            row![horizontal_space().width(Length::Fixed(menu_x)), menu_card,]
        ];

        mouse_area(container(padded).width(Length::Fill).height(Length::Fill))
            .on_press(Message::dismiss_device_menu())
            .into()
    }

    pub(super) fn view_rename_overlay(&self) -> Element<'_, Message> {
        let input = text_input("Name", &self.state.view.edit_name_text)
            .on_input(|t| Message::View(ViewMsg::EditNameText(t)))
            .on_submit(Message::View(ViewMsg::FinishEditing))
            .size(14)
            .width(Length::Fixed(250.0));

        let label = text("Rename Clip").size(14).color(th::text());

        let dialog = container(
            column![label, input]
                .spacing(8)
                .padding(16)
                .width(Length::Fixed(280.0)),
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        let centered = center(dialog).width(Length::Fill).height(Length::Fill);

        mouse_area(centered)
            .on_press(Message::View(ViewMsg::CancelEditing))
            .into()
    }

    pub(super) fn view_context_menu_overlay(&self) -> Element<'_, Message> {
        let menu = self.state.view.context_menu.as_ref().unwrap();
        let x = context_menu_x(
            menu.x,
            context_menu_width(&menu.target),
            self.state.view.window_width,
        );
        let y = menu.y;

        let menu_btn =
            |icon_char: char, label_text: String, msg: Message| -> Element<'_, Message> {
                button(
                    row![
                        icons::icon(icon_char).size(13).color(th::text()),
                        text(label_text).size(13).color(th::text())
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::menu_item(MenuOverlay::ArrangementContext, msg))
                .padding([6, 12])
                .width(Length::Fill)
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => th::bg_hover(),
                        _ => th::bg_surface(),
                    };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: th::text(),
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
            };

        let menu_items: Element<'_, Message> = match &menu.target {
            ContextMenuTarget::Clip {
                track_id,
                clip_id,
                is_note_clip,
            } => {
                let track_id = *track_id;
                let clip_id = *clip_id;
                let is_note_clip = *is_note_clip;

                let mut col = column![].spacing(0).width(Length::Fixed(220.0));

                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete".into(),
                    Message::Arrangement(ArrangementMsg::DeleteSelectedClip),
                ));
                col = col.push(menu_btn(
                    icons::COPY,
                    "Copy".into(),
                    Message::Arrangement(ArrangementMsg::CopySelectedClips),
                ));
                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Cut".into(),
                    Message::Arrangement(ArrangementMsg::CutSelectedClips),
                ));
                col = col.push(menu_btn(
                    icons::COPY,
                    "Duplicate".into(),
                    Message::Arrangement(ArrangementMsg::DuplicateSelectedClip),
                ));
                col = col.push(menu_btn(
                    icons::REPEAT,
                    "Toggle Loop".into(),
                    Message::Arrangement(ArrangementMsg::ToggleSelectedClipLoop),
                ));
                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Split Selection (Ctrl+E)".into(),
                    Message::split_selected_at_playhead(),
                ));
                col = col.push(menu_btn(
                    icons::COPY,
                    "Join Clips (Ctrl+J)".into(),
                    Message::join_selected_clips(),
                ));
                if !is_note_clip {
                    col = col.push(menu_btn(
                        icons::SKIP_BACK,
                        "Toggle Reverse".into(),
                        Message::Arrangement(ArrangementMsg::ToggleClipReverse(track_id, clip_id)),
                    ));
                    col = col.push(menu_btn(
                        icons::AUDIO_WAVEFORM,
                        "Crossfade Selected Clips".into(),
                        Message::Arrangement(ArrangementMsg::CrossfadeSelectedAudioClips),
                    ));
                }
                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Trim Track Mutes".into(),
                    Message::Arrangement(ArrangementMsg::TrimSelectedByTrackMutes),
                ));

                // Rename clip
                col = col.push(menu_btn(
                    icons::PENCIL,
                    "Rename".into(),
                    Message::View(ViewMsg::StartEditingClipName(track_id, clip_id)),
                ));

                // Bounce to audio
                col = col.push(menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Bounce to Audio".into(),
                    Message::BounceClipToAudio {
                        track_id,
                        clip_id,
                        is_note_clip,
                    },
                ));

                // Quantize (grid follows the snap setting)
                if is_note_clip {
                    col = col.push(menu_btn(
                        icons::CIRCLE_DOT,
                        format!("Quantize ({})", self.state.view.snap_grid.label()),
                        Message::PianoRoll(PianoRollMsg::QuantizeNoteClip { track_id, clip_id }),
                    ));
                } else {
                    col = col.push(menu_btn(
                        icons::CIRCLE_DOT,
                        format!("Quantize ({})", self.state.view.snap_grid.label()),
                        Message::QuantizeAudioClip { track_id, clip_id },
                    ));
                }

                col.into()
            }
            ContextMenuTarget::TimeSelection {
                start_beats,
                end_beats,
                track_id,
            } => {
                let start = *start_beats;
                let end = *end_beats;
                let mut col = column![].spacing(0).width(Length::Fixed(200.0));

                // "Create Note Clip" if track is an instrument track
                let effective_track = track_id.or(self.state.arrangement.selected_track);
                if let Some(tid) = effective_track {
                    if let Some(track) = self.state.find_track(tid) {
                        if track.kind.is_midi() {
                            col = col.push(menu_btn(
                                icons::MUSIC,
                                "Create Note Clip".into(),
                                Message::create_note_clip_from_selection(tid),
                            ));
                        }
                    }
                }

                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Split Clips at Region".into(),
                    Message::split_clips_at_region(start, end, *track_id),
                ));
                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete Clips in Region".into(),
                    Message::delete_clips_in_region(start, end, *track_id),
                ));
                col = col.push(menu_btn(
                    icons::REPEAT,
                    "Set as Loop Region".into(),
                    Message::Arrangement(ArrangementMsg::SetSelectionAsLoop),
                ));
                col = col.push(menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Bounce Selection".into(),
                    Message::BounceSelectionToAudio,
                ));

                col.into()
            }
            ContextMenuTarget::AudioClipDetail {
                location,
                track_id,
                clip_id,
                source_frame,
                timeline_frame,
                transient_marker,
                warp_marker,
            } => {
                let location = *location;
                let track_id = *track_id;
                let clip_id = *clip_id;
                let source_frame = *source_frame;
                let timeline_frame = *timeline_frame;
                let mut col = column![].spacing(0).width(Length::Fixed(200.0));

                if let Some(marker) = *warp_marker {
                    col = col.push(menu_btn(
                        icons::TRASH_2,
                        "Delete Warp marker".into(),
                        Message::Arrangement(ArrangementMsg::RemoveWarpMarker {
                            track_id,
                            clip_id,
                            source_frame: marker,
                        }),
                    ));
                } else if let Some(marker) = *transient_marker {
                    col = col.push(menu_btn(
                        icons::PLUS,
                        "Make Warp marker".into(),
                        Message::Arrangement(ArrangementMsg::AddWarpMarker {
                            track_id,
                            clip_id,
                            source_frame: marker,
                            timeline_frame,
                        }),
                    ));
                    col = col.push(menu_btn(
                        icons::TRASH_2,
                        "Delete transient".into(),
                        Message::Arrangement(ArrangementMsg::RemoveTransientMarker {
                            track_id,
                            clip_id,
                            source_frame: marker,
                        }),
                    ));
                } else {
                    col = col.push(menu_btn(
                        icons::PLUS,
                        "Add transient here".into(),
                        Message::Arrangement(ArrangementMsg::AddTransientMarker {
                            track_id,
                            clip_id,
                            source_frame,
                        }),
                    ));
                }
                col = col.push(menu_btn(
                    icons::SLIDERS_VERTICAL,
                    "Analyse · fewer markers".into(),
                    Message::DetectClipTransients {
                        location,
                        track_id,
                        clip_id,
                        detail: vibez_core::onset::TransientDetectionDetail::Fewer,
                    },
                ));
                col = col.push(menu_btn(
                    icons::SLIDERS_VERTICAL,
                    "Analyse · balanced".into(),
                    Message::DetectClipTransients {
                        location,
                        track_id,
                        clip_id,
                        detail: vibez_core::onset::TransientDetectionDetail::Balanced,
                    },
                ));
                col = col.push(menu_btn(
                    icons::SLIDERS_VERTICAL,
                    "Analyse · more markers".into(),
                    Message::DetectClipTransients {
                        location,
                        track_id,
                        clip_id,
                        detail: vibez_core::onset::TransientDetectionDetail::More,
                    },
                ));
                col = col.push(menu_btn(
                    icons::X,
                    "Clear detected markers".into(),
                    Message::Arrangement(ArrangementMsg::ReplaceDetectedTransientMarkers {
                        track_id,
                        clip_id,
                        source_frames: Vec::new(),
                    }),
                ));
                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Slice Clip at transients".into(),
                    Message::Arrangement(ArrangementMsg::SliceAudioClipAtMarkers {
                        track_id,
                        clip_id,
                        markers: AudioSliceMarkers::Transients,
                    }),
                ));
                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Slice Clip at Warp markers".into(),
                    Message::Arrangement(ArrangementMsg::SliceAudioClipAtMarkers {
                        track_id,
                        clip_id,
                        markers: AudioSliceMarkers::Warp,
                    }),
                ));
                col = col.push(menu_btn(
                    icons::MUSIC,
                    "Slice to Drum Rack…".into(),
                    Message::Arrangement(ArrangementMsg::RequestSliceAudioClipToDrumRack {
                        track_id,
                        clip_id,
                    }),
                ));
                col.into()
            }
            ContextMenuTarget::ArrangementEmpty => column![
                menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Add Audio Track".into(),
                    Message::Arrangement(ArrangementMsg::AddTrack),
                ),
                menu_btn(
                    icons::MUSIC,
                    "Add MIDI Track".into(),
                    Message::Arrangement(ArrangementMsg::AddInstrumentTrack),
                ),
            ]
            .spacing(0)
            .width(Length::Fixed(200.0))
            .into(),
        };

        let menu_container = container(menu_items)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .padding(4);

        // Position menu at (x, y) using spacers in a column+row layout
        let positioned = column![
            vertical_space().height(Length::Fixed(y)),
            row![horizontal_space().width(Length::Fixed(x)), menu_container,]
        ];

        // Full-screen click-eating backdrop
        mouse_area(
            container(positioned)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::dismiss_menu(MenuOverlay::ArrangementContext))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        context_menu_x, drum_rack_slice_can_create, project_track_deletion_list_height,
        CONTEXT_MENU_CLIP_WIDTH,
    };

    #[test]
    fn deletion_location_list_grows_with_content_then_caps() {
        assert_eq!(project_track_deletion_list_height(0), 28.0);
        assert_eq!(project_track_deletion_list_height(1), 28.0);
        assert_eq!(project_track_deletion_list_height(2), 60.0);
        assert_eq!(project_track_deletion_list_height(8), 120.0);
    }

    #[test]
    fn clip_context_menu_stays_full_width_at_the_right_window_edge() {
        let x = context_menu_x(1_862.0, CONTEXT_MENU_CLIP_WIDTH, 1_920.0);

        assert_eq!(x, 1_676.0);
    }

    #[test]
    fn drum_rack_slice_dialog_never_accepts_empty_or_overflowing_racks() {
        assert!(!drum_rack_slice_can_create(0));
        assert!(drum_rack_slice_can_create(1));
        assert!(drum_rack_slice_can_create(16));
        assert!(!drum_rack_slice_can_create(17));
    }
}
