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

use super::*;

impl App {
    pub(super) fn view_settings_modal(&self) -> Element<'_, Message> {
        let title = text("Settings").size(18).color(th::ACCENT);
        let close_btn = button(icons::icon(icons::X).size(14).color(th::TEXT_DIM))
            .on_press(Message::CloseSettings)
            .padding([4, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT_DIM,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let header = row![title, horizontal_space(), close_btn].align_y(iced::Alignment::Center);

        // -- Tab bar --
        let make_tab_btn = |label: &'static str, tab: SettingsTab, is_active: bool| {
            let color = if is_active { th::ACCENT } else { th::TEXT_DIM };
            button(text(label).size(13).color(color))
                .on_press(Message::SelectSettingsTab(tab))
                .padding([6, 16])
                .style(move |_theme: &Theme, status| {
                    let bg = if is_active {
                        None
                    } else {
                        match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::BG_HOVER.into())
                            }
                            _ => None,
                        }
                    };
                    button::Style {
                        background: bg,
                        text_color: color,
                        border: iced::Border {
                            color: if is_active {
                                th::ACCENT
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
        ]
        .spacing(0);

        // -- Tab body --
        let tab_body: Element<'_, Message> = match self.state.settings_tab {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
            SettingsTab::Dropbox => self.view_settings_dropbox_tab(),
            SettingsTab::Warping => self.view_settings_warping_tab(),
        };

        let content = column![
            header,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            tab_bar,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            tab_body,
        ]
        .spacing(8)
        .padding(20)
        .width(Length::Fixed(480.0));

        let dialog = container(content).style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
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
        let buf_label = text("Buffer Size").size(14).color(th::TEXT);
        let buf_hint = text("Lower = less latency, higher = more CPU headroom")
            .size(11)
            .color(th::TEXT_DIM);

        let sizes: &[u32] = &[64, 128, 256, 512, 1024, 2048, 4096];
        let mut buf_row = row![].spacing(4);
        for &size in sizes {
            let is_selected = self.state.settings_buffer_size == size;
            let label = format!("{size}");
            let btn = button(text(label).size(11).color(if is_selected {
                th::TEXT
            } else {
                th::TEXT_DIM
            }))
            .on_press(Message::SetBufferSize(size))
            .padding([6, 10])
            .style(move |_theme: &Theme, status| {
                if is_selected {
                    button::Style {
                        background: Some(th::ACCENT.into()),
                        text_color: th::TEXT,
                        border: iced::Border {
                            color: th::ACCENT,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT_DIM,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                }
            });
            buf_row = buf_row.push(btn);
        }

        let sr_label = text("Sample Rate").size(14).color(th::TEXT);
        let sr_value = text(format!("{} Hz", self.state.transport.sample_rate))
            .size(13)
            .color(th::TEXT_DIM);

        // ---- MIDI input picker ----
        let midi_label = text("MIDI Input").size(14).color(th::TEXT);
        let midi_hint = text(
            "External MIDI routes to the currently selected instrument track. \
             Plug your keyboard or Push in, hit Rescan, then pick the port.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let current_port_line: Element<'_, Message> = match self.midi_input.as_ref() {
            Some(h) => text(format!("Connected: {}", h.port_name))
                .size(12)
                .color(th::ACCENT)
                .into(),
            None => text("Not connected").size(12).color(th::TEXT_DIM).into(),
        };

        let rescan_btn = button(text("Rescan ports").size(11).color(th::TEXT))
            .on_press(Message::RescanMidiInputs)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let disconnect_btn = button(text("Disconnect").size(11).color(th::TEXT_DIM))
            .on_press(Message::CloseMidiInput)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
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
                .color(if is_current { th::ACCENT } else { th::TEXT }),
            )
            .on_press(Message::OpenMidiInput(label))
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if is_current {
                    Some(th::BG_HOVER.into())
                } else {
                    match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    }
                };
                button::Style {
                    background: bg,
                    text_color: if is_current { th::ACCENT } else { th::TEXT },
                    border: iced::Border {
                        color: if is_current {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
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
                    background: Some(th::BORDER.into()),
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
        let plugin_title = text("Plugin Library").size(14).color(th::TEXT);

        // Default paths checkbox
        let default_paths_label = if self.state.plugin_settings.scan_default_paths {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::ACCENT)
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::TEXT_DIM)
        };
        let default_paths_btn = button(
            row![
                default_paths_label,
                text("Scan default system paths").size(12).color(th::TEXT)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleScanDefaultPaths)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::TEXT,
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
                .color(th::TEXT_DIM);
            let remove_btn = button(icons::icon(icons::X).size(10).color(th::DANGER))
                .on_press(Message::RemovePluginScanPath(i))
                .padding([2, 6])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::DANGER,
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
                icons::icon(icons::PLUS).size(12).color(th::ACCENT),
                text("Add Path").size(12).color(th::ACCENT)
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddPluginScanPath)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::ACCENT,
                border: iced::Border {
                    color: th::ACCENT_DIM,
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
        let scan_btn = button(text(scan_label).size(12).color(th::TEXT))
            .on_press(Message::ScanPlugins)
            .padding([8, 16])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::ACCENT_DIM.into())
                    }
                    _ => Some(th::BG_ELEVATED.into()),
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT,
                    border: iced::Border {
                        color: th::BORDER,
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
                .color(th::TEXT_DIM)
        } else {
            text(format!("{cache_count} plugins cached"))
                .size(11)
                .color(th::TEXT_DIM)
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
        let title = text("Dropbox").size(14).color(th::TEXT);
        let hint = text(
            "Register an app at https://www.dropbox.com/developers/apps \
            (Scoped access, Full Dropbox). Paste the App key below.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let app_key_input = text_input("App key", &self.state.browser.dropbox.app_key_input)
            .on_input(|s| Message::Browser(BrowserMsg::SetDropboxAppKey(s)))
            .on_submit(Message::SaveDropboxAppKey)
            .size(13)
            .width(Length::Fill);
        let save_key_btn = button(text("Save").size(12).color(th::TEXT))
            .on_press(Message::SaveDropboxAppKey)
            .padding([6, 12])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let key_row = row![app_key_input, save_key_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let account_line: Element<'_, Message> = if self.state.browser.dropbox.connected {
            let email = self
                .state
                .browser
                .dropbox
                .account_email
                .clone()
                .unwrap_or_else(|| "connected".into());
            text(format!("Connected: {email}"))
                .size(12)
                .color(th::ACCENT)
                .into()
        } else if self.state.browser.dropbox.auth_in_progress {
            text("Waiting for browser authorisation...")
                .size(12)
                .color(th::TEXT_DIM)
                .into()
        } else {
            text("Not connected").size(12).color(th::TEXT_DIM).into()
        };

        let can_connect =
            self.state.browser.dropbox.has_app_key && !self.state.browser.dropbox.auth_in_progress;
        let connect_label = if self.state.browser.dropbox.auth_in_progress {
            "Connecting..."
        } else if self.state.browser.dropbox.connected {
            "Reconnect"
        } else {
            "Connect"
        };
        let connect_btn = {
            let mut btn = button(text(connect_label).size(12).color(th::ACCENT));
            if can_connect {
                btn = btn.on_press(Message::ConnectDropbox);
            }
            btn.padding([6, 12]).style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            })
        };

        let disconnect_btn: Element<'_, Message> = if self.state.browser.dropbox.connected {
            button(text("Disconnect").size(12).color(th::TEXT_DIM))
                .on_press(Message::DisconnectDropbox)
                .padding([6, 12])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT_DIM,
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
        } else {
            horizontal_space().width(Length::Shrink).into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.browser.dropbox.last_error.clone() {
                text(err).size(11).color(th::DANGER).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        column![
            title,
            hint,
            key_row,
            account_line,
            row![connect_btn, disconnect_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            error_line,
        ]
        .spacing(10)
        .into()
    }

    pub(super) fn view_settings_warping_tab(&self) -> Element<'_, Message> {
        let title = text("Sample Warping").size(14).color(th::TEXT);
        let hint = text(
            "Auto-warp detects BPM of each dropped sample and time-stretches it to \
             the project tempo, preserving pitch. Turn this off to keep samples at their \
             original speed.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let toggle_icon = if self.state.auto_warp_on_import {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::ACCENT)
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::TEXT_DIM)
        };
        let toggle_btn = button(
            row![
                toggle_icon,
                text("Auto-warp samples on import").size(12).color(th::TEXT)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleAutoWarpOnImport)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::TEXT,
            border: iced::Border::default(),
            ..Default::default()
        });

        let conf = self.state.warp_confidence_threshold;
        let conf_label = text("Detection confidence threshold")
            .size(12)
            .color(th::TEXT);
        let conf_value = text(format!("{:.2}", conf)).size(12).color(th::TEXT_DIM);
        let conf_hint = text(
            "Higher = only warp when the detector is very sure. \
             Lower = warp even ambiguous clips.",
        )
        .size(11)
        .color(th::TEXT_DIM);
        let conf_slider = slider(0.0..=1.0, conf, Message::SetWarpConfidenceThreshold).step(0.05);

        let rewarp_btn = button(
            text("Re-warp all clips to project tempo")
                .size(12)
                .color(th::TEXT),
        )
        .on_press(Message::RewarpAllClips)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::ACCENT_DIM,
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
                    background: Some(th::BORDER.into()),
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
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            rewarp_btn,
        ]
        .spacing(10)
        .into()
    }
}
