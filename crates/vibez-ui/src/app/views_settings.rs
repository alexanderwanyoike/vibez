//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, slider, text, text_input,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::browser::BrowserMsg;

use crate::icons;
use crate::message::Message;
use crate::state::SettingsTab;
use crate::theme as th;

use super::views_browser_style::browser_utility_action_style;
use super::*;

impl App {
    pub(super) fn view_settings_modal(&self) -> Element<'_, Message> {
        let title = text("Settings").size(18).color(th::accent());
        let close_btn = button(icons::icon(icons::X).size(14).color(th::text_dim()))
            .on_press(Message::CloseSettings)
            .padding([4, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let header = row![title, horizontal_space(), close_btn].align_y(iced::Alignment::Center);

        // -- Tab bar --
        let make_tab_btn = |label: &'static str, tab: SettingsTab, is_active: bool| {
            let color = if is_active {
                th::accent()
            } else {
                th::text_dim()
            };
            button(text(label).size(13).color(color))
                .on_press(Message::SelectSettingsTab(tab))
                .padding([6, 10])
                .style(move |_theme: &Theme, status| {
                    let bg = if is_active {
                        None
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
                        text_color: color,
                        border: iced::Border {
                            color: if is_active {
                                th::accent()
                            } else {
                                Color::TRANSPARENT
                            },
                            width: if is_active { 2.0 } else { 0.0 },
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }
                })
        };

        let active = self.state.settings_tab;
        let tab_bar = row![
            make_tab_btn("Audio", SettingsTab::Audio, active == SettingsTab::Audio),
            make_tab_btn(
                "Plugins",
                SettingsTab::Plugins,
                active == SettingsTab::Plugins
            ),
            make_tab_btn(
                "Dropbox",
                SettingsTab::Dropbox,
                active == SettingsTab::Dropbox
            ),
            make_tab_btn(
                "Warping",
                SettingsTab::Warping,
                active == SettingsTab::Warping
            ),
            make_tab_btn(
                "Perform",
                SettingsTab::Perform,
                active == SettingsTab::Perform
            ),
            make_tab_btn(
                "Appearance",
                SettingsTab::Appearance,
                active == SettingsTab::Appearance
            ),
        ]
        .spacing(0);

        // -- Tab body --
        let tab_body: Element<'_, Message> = match self.state.settings_tab {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
            SettingsTab::Dropbox => self.view_settings_dropbox_tab(),
            SettingsTab::Warping => self.view_settings_warping_tab(),
            SettingsTab::Perform => self.view_settings_perform_tab(),
            SettingsTab::Appearance => self.view_settings_appearance_tab(),
        };

        let content = column![
            header,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }
            ),
            tab_bar,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }
            ),
            tab_body,
        ]
        .spacing(8)
        .padding(20)
        .width(Length::Fixed(480.0));

        let dialog = container(content).style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        // Centered overlay with dimmed background
        mouse_area(
            container(center(dialog).width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5).into()),
                    ..Default::default()
                }),
        )
        .on_press(Message::CloseSettings)
        .into()
    }

    pub(super) fn view_settings_audio_tab(&self) -> Element<'_, Message> {
        let buf_label = text("Buffer Size").size(14).color(th::text());
        let buf_hint = text("Lower = less latency, higher = more CPU headroom")
            .size(11)
            .color(th::text_dim());

        let sizes: &[u32] = &[64, 128, 256, 512, 1024, 2048, 4096];
        let mut buf_row = row![].spacing(4);
        for &size in sizes {
            let is_selected = self.state.settings_buffer_size == size;
            let label = format!("{size}");
            let btn = button(text(label).size(11).color(if is_selected {
                th::text()
            } else {
                th::text_dim()
            }))
            .on_press(Message::SetBufferSize(size))
            .padding([6, 10])
            .style(move |_theme: &Theme, status| {
                if is_selected {
                    button::Style {
                        background: Some(th::accent().into()),
                        text_color: th::text(),
                        border: iced::Border {
                            color: th::accent(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text_dim(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                }
            });
            buf_row = buf_row.push(btn);
        }

        let sr_label = text("Sample Rate").size(14).color(th::text());
        let sr_value = text(format!("{} Hz", self.state.transport.sample_rate))
            .size(13)
            .color(th::text_dim());

        // ---- MIDI input picker ----
        let midi_label = text("MIDI Input").size(14).color(th::text());
        let midi_hint = text(
            "External MIDI routes to the currently selected instrument track. \
             Plug your keyboard or Push in, hit Rescan, then pick the port.",
        )
        .size(11)
        .color(th::text_dim());

        let current_port_line: Element<'_, Message> = match self.midi_input.as_ref() {
            Some(h) => text(format!("Connected: {}", h.port_name))
                .size(12)
                .color(th::accent())
                .into(),
            None => text("Not connected").size(12).color(th::text_dim()).into(),
        };

        let rescan_btn = button(text("Rescan ports").size(11).color(th::text()))
            .on_press(Message::RescanMidiInputs)
            .padding([4, 10])
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
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let disconnect_btn = button(text("Disconnect").size(11).color(th::text_dim()))
            .on_press(Message::CloseMidiInput)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text_dim(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let midi_actions = row![rescan_btn, disconnect_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        let mut port_list = column![].spacing(3);
        for name in &self.midi_input_ports {
            let is_current = self
                .midi_input
                .as_ref()
                .map(|h| h.port_name == *name)
                .unwrap_or(false);
            let label = name.clone();
            let port_btn = button(
                text(if is_current {
                    format!("● {name}")
                } else {
                    name.clone()
                })
                .size(11)
                .color(if is_current { th::accent() } else { th::text() }),
            )
            .on_press(Message::OpenMidiInput(label))
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if is_current {
                    Some(th::bg_hover().into())
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
                    text_color: if is_current { th::accent() } else { th::text() },
                    border: iced::Border {
                        color: if is_current {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
            port_list = port_list.push(port_btn);
        }

        column![
            buf_label,
            buf_hint,
            buf_row,
            sr_label,
            sr_value,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }
            ),
            midi_label,
            midi_hint,
            current_port_line,
            midi_actions,
            port_list,
        ]
        .spacing(8)
        .into()
    }

    pub(super) fn view_settings_plugins_tab(&self) -> Element<'_, Message> {
        // Plugin section header
        let plugin_title = text("Plugin Library").size(14).color(th::text());

        // Default paths checkbox
        let default_paths_label = if self.state.plugin_settings.scan_default_paths {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::accent())
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::text_dim())
        };
        let default_paths_btn = button(
            row![
                default_paths_label,
                text("Scan default system paths").size(12).color(th::text())
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleScanDefaultPaths)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::text(),
            border: iced::Border::default(),
            ..Default::default()
        });

        // Scan paths list
        let mut paths_col = column![].spacing(4);
        for (i, path) in self
            .state
            .plugin_settings
            .extra_scan_paths
            .iter()
            .enumerate()
        {
            let path_text = text(path.display().to_string())
                .size(11)
                .color(th::text_dim());
            let remove_btn = button(icons::icon(icons::X).size(10).color(th::danger()))
                .on_press(Message::RemovePluginScanPath(i))
                .padding([2, 6])
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
            paths_col = paths_col.push(
                row![path_text, horizontal_space(), remove_btn]
                    .align_y(iced::Alignment::Center)
                    .spacing(4),
            );
        }

        let add_path_btn = button(
            row![
                icons::icon(icons::PLUS).size(12).color(th::accent()),
                text("Add Path").size(12).color(th::accent())
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddPluginScanPath)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
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
        });

        // Scan button
        let scan_label = if self.state.plugin_scan_in_progress {
            "Scanning..."
        } else {
            "Scan Plugins"
        };
        let scan_btn = button(text(scan_label).size(12).color(th::text()))
            .on_press(Message::ScanPlugins)
            .padding([8, 16])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::accent_dim().into())
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

        // Status
        let cache_count = self.state.plugin_settings.cache.len();
        let status = if !self.state.plugin_scan_status.is_empty() {
            text(&self.state.plugin_scan_status)
                .size(11)
                .color(th::text_dim())
        } else {
            text(format!("{cache_count} plugins cached"))
                .size(11)
                .color(th::text_dim())
        };

        column![
            plugin_title,
            default_paths_btn,
            paths_col,
            row![add_path_btn, horizontal_space(), scan_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            status,
        ]
        .spacing(8)
        .into()
    }

    pub(super) fn view_settings_dropbox_tab(&self) -> Element<'_, Message> {
        let title = text("Dropbox").size(14).color(th::text());
        let hint = text(
            "Register an app at https://www.dropbox.com/developers/apps \
            (Scoped access, Full Dropbox). Paste the App key below.",
        )
        .size(11)
        .color(th::text_dim());

        let app_key_input = text_input("App key", &self.state.browser.remote.app_key_input)
            .on_input(|s| Message::Browser(BrowserMsg::SetDropboxAppKey(s)))
            .on_submit(Message::SaveDropboxAppKey)
            .size(13)
            .width(Length::Fill);
        let save_key_btn = button(text("Save").size(12).color(th::text()))
            .on_press(Message::SaveDropboxAppKey)
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
            });

        let key_row = row![app_key_input, save_key_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let account_line: Element<'_, Message> = if self.state.browser.remote.connected {
            let email = self
                .state
                .browser
                .remote
                .account_email
                .clone()
                .unwrap_or_else(|| "connected".into());
            text(format!("Connected: {email}"))
                .size(12)
                .color(th::accent())
                .into()
        } else if self.state.browser.remote.auth_in_progress {
            text("Waiting for browser authorisation...")
                .size(12)
                .color(th::text_dim())
                .into()
        } else {
            text("Not connected").size(12).color(th::text_dim()).into()
        };

        let can_connect =
            self.state.browser.remote.has_app_key && !self.state.browser.remote.auth_in_progress;
        let connect_label = if self.state.browser.remote.auth_in_progress {
            "Connecting..."
        } else if self.state.browser.remote.connected {
            "Reconnect"
        } else {
            "Connect"
        };
        let connect_btn = {
            let mut btn = button(text(connect_label).size(12).color(th::accent()));
            if can_connect {
                btn = btn.on_press(Message::ConnectDropbox);
            }
            btn.padding([6, 12]).style(|_theme: &Theme, status| {
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
            })
        };

        let disconnect_btn: Element<'_, Message> = if self.state.browser.remote.connected {
            button(text("Disconnect").size(12).color(th::text_dim()))
                .on_press(Message::DisconnectDropbox)
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
                        text_color: th::text_dim(),
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
        } else {
            horizontal_space().width(Length::Shrink).into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.browser.remote.last_error.clone() {
                text(err).size(11).color(th::danger()).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        let budget_gib =
            self.state.browser.remote.cache_budget_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
        let cache_usage = text(format!(
            "{} across {} item(s)",
            format_settings_bytes(self.state.browser.remote.cache_usage_bytes),
            self.state.browser.remote.cache_entries
        ))
        .size(11)
        .color(th::text_dim());
        let budget = slider(1.0..=500.0, budget_gib, Message::SetMediaCacheBudgetGiB)
            .step(1.0_f32)
            .width(Length::Fill);
        let eviction_enabled = self.state.browser.remote.cache_automatic_eviction;
        let eviction = button(
            text(if eviction_enabled {
                "LRU EVICTION ON"
            } else {
                "LRU EVICTION OFF"
            })
            .size(10)
            .color(if eviction_enabled {
                th::accent()
            } else {
                th::text_dim()
            }),
        )
        .on_press(Message::ToggleMediaCacheAutomaticEviction)
        .padding([5, 8])
        .style(browser_utility_action_style);
        let clear = button(text("CLEAR CACHE").size(10).color(th::text_dim()))
            .on_press(Message::ClearMediaCache)
            .padding([5, 8])
            .style(browser_utility_action_style);
        let cache_error: Element<'_, Message> = self
            .state
            .browser
            .remote
            .cache_error
            .as_ref()
            .map(|error| text(error).size(10).color(th::danger()).into())
            .unwrap_or_else(|| horizontal_space().width(Length::Shrink).into());

        column![
            title,
            hint,
            key_row,
            account_line,
            row![connect_btn, disconnect_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            error_line,
            text("MEDIA CACHE").size(10).color(th::text_muted()),
            row![
                cache_usage,
                horizontal_space(),
                text(format!("{budget_gib:.0} GiB budget"))
                    .size(11)
                    .color(th::text_dim())
            ]
            .align_y(iced::Alignment::Center),
            budget,
            row![eviction, clear]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            cache_error,
        ]
        .spacing(10)
        .into()
    }

    pub(super) fn view_settings_warping_tab(&self) -> Element<'_, Message> {
        let title = text("Sample Warping").size(14).color(th::text());
        let hint = text(
            "Auto-warp detects BPM of each dropped sample and time-stretches it to \
             the project tempo, preserving pitch. Turn this off to keep samples at their \
             original speed.",
        )
        .size(11)
        .color(th::text_dim());

        let toggle_icon = if self.state.auto_warp_on_import {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::accent())
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::text_dim())
        };
        let toggle_btn = button(
            row![
                toggle_icon,
                text("Auto-warp samples on import")
                    .size(12)
                    .color(th::text())
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleAutoWarpOnImport)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::text(),
            border: iced::Border::default(),
            ..Default::default()
        });

        let conf = self.state.warp_confidence_threshold;
        let conf_label = text("Detection confidence threshold")
            .size(12)
            .color(th::text());
        let conf_value = text(format!("{:.2}", conf)).size(12).color(th::text_dim());
        let conf_hint = text(
            "Higher = only warp when the detector is very sure. \
             Lower = warp even ambiguous clips.",
        )
        .size(11)
        .color(th::text_dim());
        let conf_slider =
            slider(0.0..=1.0, conf, Message::SetWarpConfidenceThreshold).step(0.05_f32);

        let rewarp_btn = button(
            text("Re-warp all clips to project tempo")
                .size(12)
                .color(th::text()),
        )
        .on_press(Message::RewarpAllClips)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
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
        });

        column![
            title,
            hint,
            toggle_btn,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }
            ),
            conf_label,
            conf_hint,
            row![conf_slider, conf_value]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::border().into()),
                    ..Default::default()
                }
            ),
            rewarp_btn,
        ]
        .spacing(10)
        .into()
    }

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
                text("No .vzt themes found — save one below or drop files in the folder.")
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

fn format_settings_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else if bytes as f64 >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else {
        format!("{:.1} KiB", bytes as f64 / KIB)
    }
}
