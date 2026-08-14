//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, pick_list, row, scrollable,
    slider, text,
};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::SettingsTab;
use crate::theme as th;

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
            // A tab label must never wrap: when the bar runs out of room,
            // wrapping turns the last tab into a one-letter-per-line
            // column. Clipping is the survivable failure.
            button(
                text(label)
                    .size(13)
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .on_press(Message::SelectSettingsTab(tab))
            .padding([6, 8])
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
                "Project",
                SettingsTab::Project,
                active == SettingsTab::Project
            ),
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
            make_tab_btn(
                "Updates",
                SettingsTab::Updates,
                active == SettingsTab::Updates
            ),
        ]
        .spacing(0);

        // -- Tab body --
        let tab_body: Element<'_, Message> = match self.state.settings_tab {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Project => self.view_settings_project_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
            SettingsTab::Dropbox => self.view_settings_dropbox_tab(),
            SettingsTab::Warping => self.view_settings_warping_tab(),
            SettingsTab::Perform => self.view_settings_perform_tab(),
            SettingsTab::Appearance => self.view_settings_appearance_tab(),
            SettingsTab::Updates => self.view_settings_updates_tab(),
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
        // Wide enough for all eight tab labels on one line with room to
        // spare; still comfortably inside the 900px minimum window. Grow
        // this before adding a ninth tab.
        .width(Length::Fixed(600.0));

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
        let hardware_label = text("Audio Hardware").size(14).color(th::text());
        let hardware_hint =
            text("Hardware choices are application-wide. Projects keep their own musical state.")
                .size(11)
                .color(th::text_dim());
        let catalog_error: Element<'_, Message> = self
            .state
            .audio_settings
            .catalog_error
            .as_ref()
            .map(|error| {
                text(format!("Device scan failed: {error}"))
                    .size(11)
                    .color(th::danger())
                    .into()
            })
            .unwrap_or_else(|| column![].into());

        let output_picker = pick_list(
            self.state.audio_settings.output_choices(),
            Some(self.state.audio_settings.selected_output_choice()),
            Message::SelectAudioOutput,
        )
        .placeholder("No Audio Output")
        .width(Length::Fill);
        let input_picker = pick_list(
            self.state.audio_settings.input_choices(),
            Some(self.state.audio_settings.selected_input_choice()),
            Message::SelectAudioInput,
        )
        .placeholder("No Audio Input")
        .width(Length::Fill);

        let output_status = match &self.state.audio_stream_health {
            AudioStreamHealth::Running => {
                let active = self
                    .state
                    .audio_settings
                    .active_output_name
                    .as_deref()
                    .unwrap_or("System Default");
                text(format!("Running: {active}"))
                    .size(11)
                    .color(th::accent())
            }
            AudioStreamHealth::Rebuilding => text("Applying Audio Configuration…")
                .size(11)
                .color(th::text_dim()),
            AudioStreamHealth::Error(cause) => text(format!("Output error: {cause}"))
                .size(11)
                .color(th::danger()),
        };
        let input_status = text(self.state.audio_settings.input_description())
            .size(11)
            .color(if self.state.audio_settings.selected_input().is_some() {
                th::accent()
            } else {
                th::text_dim()
            });

        let rescan_audio = button(text("Rescan").size(11).color(th::text()))
            .on_press(Message::RescanAudioDevices)
            .padding([5, 10]);
        let reconnect_audio = button(text("Reconnect").size(11).color(th::text()))
            .on_press(Message::ReconnectAudioOutput)
            .padding([5, 10]);

        let buf_label = text("Buffer Size").size(14).color(th::text());
        let buf_hint = text("Lower = less latency, higher = more CPU headroom")
            .size(11)
            .color(th::text_dim());

        let sizes = self.state.audio_settings.buffer_size_choices();
        let mut buf_row = row![].spacing(4);
        for size in sizes {
            let is_selected = self.state.audio_settings.buffer_size == size;
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
        let sr_picker = pick_list(
            self.state.audio_settings.sample_rate_choices(),
            Some(crate::domains::audio_settings::AudioSampleRate(
                self.state.audio_settings.sample_rate,
            )),
            Message::SetAudioSampleRate,
        )
        .width(Length::Fixed(150.0));

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

        let body = column![
            hardware_label,
            hardware_hint,
            catalog_error,
            text("Audio Output").size(12).color(th::text_dim()),
            output_picker,
            output_status,
            text("Audio Input").size(12).color(th::text_dim()),
            input_picker,
            input_status,
            row![rescan_audio, reconnect_audio].spacing(6),
            sr_label,
            sr_picker,
            buf_label,
            buf_hint,
            buf_row,
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
        .spacing(8);
        scrollable(body).height(Length::Fixed(440.0)).into()
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
}

/// Shared with the Dropbox tab, which reports Media Cache usage in the
/// same units.
pub(super) fn format_settings_bytes(bytes: u64) -> String {
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
