//! Modal overlays: context menus, file menu, rename prompt.

//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, scrollable, text,
    text_input, vertical_space,
};
use iced::{Element, Length, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::view::ViewMsg;
use vibez_core::effect::EffectType;
use vibez_core::midi::InstrumentKind;
use vibez_plugin_host::{PluginCategory, PluginFormat};

use crate::icons;
use crate::message::Message;
use crate::state::ContextMenuTarget;
use crate::theme as th;

use super::*;

impl App {
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
            .on_press(message)
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
                "Paste at Playhead",
                "Ctrl+V",
                Message::Arrangement(ArrangementMsg::PasteClipsAtPlayhead),
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
        .on_press(Message::View(ViewMsg::DismissEditMenu))
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
            .on_press(msg)
            .padding([8, 16])
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
        .on_press(Message::OpenSettings)
        .padding([8, 16])
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

        let menu_content = column![new_btn]
            .spacing(2)
            .push(open_btn)
            .push(save_btn)
            .push(save_as_btn)
            .push(export_btn)
            .push(settings_btn)
            .padding(4)
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

        // Position below the header, near the File button
        let padded = column![
            vertical_space().height(Length::Fixed(42.0)),
            row![horizontal_space().width(Length::Fixed(60.0)), menu_card,]
        ];

        mouse_area(container(padded).width(Length::Fill).height(Length::Fill))
            .on_press(Message::Project(ProjectMsg::DismissFileMenu))
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
        let x = menu.x;
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
                .on_press(msg)
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

                let mut col = column![].spacing(0).width(Length::Fixed(200.0));

                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete".into(),
                    Message::Arrangement(ArrangementMsg::DeleteSelectedClip),
                ));
                col = col.push(menu_btn(
                    icons::COPY,
                    "Duplicate".into(),
                    Message::Arrangement(ArrangementMsg::DuplicateSelectedClip),
                ));

                // Split at playhead
                let playhead_beats = self.state.position_beats();
                if is_note_clip {
                    col = col.push(menu_btn(
                        icons::SCISSORS,
                        "Split at Playhead".into(),
                        Message::split_note_clip(track_id, clip_id, playhead_beats),
                    ));
                } else {
                    let split_sample = self.state.transport.position_samples;
                    col = col.push(menu_btn(
                        icons::SCISSORS,
                        "Split at Playhead".into(),
                        Message::split_audio_clip(track_id, clip_id, split_sample),
                    ));
                }

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
        .on_press(Message::View(ViewMsg::DismissContextMenu))
        .into()
    }
}
