//! The Settings sheet: a section sidebar beside the chosen section, plus
//! the Plugins and Warping sections.

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, scrollable, slider, text,
};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::SettingsTab;
use crate::theme as th;

use super::*;

const SHEET_W: f32 = 900.0;
const SHEET_H: f32 = 640.0;
const SHEET_MARGIN: f32 = 24.0;
const SHEET_RADIUS: f32 = 12.0;
const NAV_W: f32 = 200.0;
const NAV_ROW_H: f32 = 30.0;

/// A Settings section as the sidebar lists it.
struct Section {
    tab: SettingsTab,
    label: &'static str,
    icon: char,
    hint: &'static str,
}

const SECTIONS: [Section; 8] = [
    Section {
        tab: SettingsTab::Audio,
        label: "Audio",
        icon: icons::VOLUME_2,
        hint: "Audio and MIDI hardware for this computer, never saved with a project.",
    },
    Section {
        tab: SettingsTab::Project,
        label: "Project",
        icon: icons::FILE,
        hint: "How projects save and how monitored tracks are timed.",
    },
    Section {
        tab: SettingsTab::Plugins,
        label: "Plugins",
        icon: icons::PLUG,
        hint: "Where Vibez looks for VST3 and CLAP plugins.",
    },
    Section {
        tab: SettingsTab::Dropbox,
        label: "Dropbox",
        icon: icons::CLOUD,
        hint: "Browse and audition your Dropbox, and how much of it stays on disk.",
    },
    Section {
        tab: SettingsTab::Warping,
        label: "Warping",
        icon: icons::AUDIO_LINES,
        hint: "How dropped samples follow the project tempo.",
    },
    Section {
        tab: SettingsTab::Perform,
        label: "Perform",
        icon: icons::LAYOUT_GRID,
        hint: "The computer keys that play Perform pads, and how hard they play.",
    },
    Section {
        tab: SettingsTab::Appearance,
        label: "Appearance",
        icon: icons::PALETTE,
        hint: "How Vibez looks. Changes apply at once and are saved for you, not in the project.",
    },
    Section {
        tab: SettingsTab::Updates,
        label: "Updates",
        icon: icons::ROTATE_CW,
        hint: "Whether Vibez looks for a newer release, and what it found.",
    },
];

impl App {
    pub(super) fn view_settings_modal(&self) -> Element<'_, Message> {
        let sheet = settings_sheet_size(iced::Size::new(
            self.state.view.window_width,
            self.state.view.window_height,
        ));
        let active = self.state.settings_tab;

        let mut nav = column![
            container(text("Settings").size(13).font(bold()).color(th::text())).padding(
                iced::Padding {
                    top: 0.0,
                    right: 8.0,
                    bottom: 12.0,
                    left: 8.0,
                }
            )
        ]
        .spacing(2);
        for section in &SECTIONS {
            let selected = section.tab == active;
            nav = nav.push(
                button(
                    container(
                        row![
                            icons::icon(section.icon).size(14).color(if selected {
                                th::accent()
                            } else {
                                th::text_dim()
                            }),
                            text(section.label)
                                .size(12)
                                .font(if selected { semibold() } else { medium() })
                                .color(th::text())
                                .wrapping(iced::widget::text::Wrapping::None),
                        ]
                        .spacing(10)
                        .align_y(iced::Alignment::Center),
                    )
                    .center_y(NAV_ROW_H),
                )
                .on_press(Message::SelectSettingsTab(section.tab))
                .padding([0, 10])
                .width(Length::Fill)
                .style(move |_theme: &Theme, status| settings_tab_style(selected, status)),
            );
        }
        let nav = container(nav)
            .padding([18, 10])
            .width(NAV_W)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(nav_background().into()),
                ..Default::default()
            });

        let section = SECTIONS
            .iter()
            .find(|section| section.tab == active)
            .unwrap_or(&SECTIONS[0]);
        let close = button(
            container(icons::icon(icons::X).size(14))
                .center_x(28.0)
                .center_y(28.0),
        )
        .on_press(Message::CloseSettings)
        .padding(0)
        .style(|_theme: &Theme, status| button::Style {
            border: iced::Border {
                radius: 7.0.into(),
                ..Default::default()
            },
            ..crate::widgets::control_style::styled(None, status)
        });
        let header = row![
            column![
                text(section.label).size(18).font(bold()).color(th::text()),
                text(section.hint).size(11.5).color(th::text_muted()),
            ]
            .spacing(4)
            .width(Length::Fill),
            close
        ]
        .align_y(iced::Alignment::Start);

        let tab_body: Element<'_, Message> = match active {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Project => self.view_settings_project_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
            SettingsTab::Dropbox => self.view_settings_dropbox_tab(),
            SettingsTab::Warping => self.view_settings_warping_tab(),
            SettingsTab::Perform => self.view_settings_perform_tab(),
            SettingsTab::Appearance => self.view_settings_appearance_tab(),
            SettingsTab::Updates => self.view_settings_updates_tab(),
        };
        let body = scrollable(container(tab_body).padding(iced::Padding {
            right: 12.0,
            ..Default::default()
        }))
        .direction(scrollable::Direction::Vertical(
            crate::widgets::quiet_scroll::scrollbar(),
        ))
        .style(crate::widgets::quiet_scroll::style)
        .height(Length::Fill);
        let content = column![header, body]
            .spacing(16)
            .padding(iced::Padding {
                top: 22.0,
                right: 14.0,
                bottom: 22.0,
                left: 26.0,
            })
            .width(Length::Fill)
            .height(Length::Fill);

        let sheet = container(row![nav, content])
            .width(sheet.width)
            .height(sheet.height)
            .clip(true)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::hairline(th::bg_surface()),
                    width: 1.0,
                    radius: SHEET_RADIUS.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, if th::is_light() { 0.3 } else { 0.6 }),
                    offset: iced::Vector::new(0.0, 24.0),
                    blur_radius: 60.0,
                },
                ..Default::default()
            });
        // Presses on the sheet itself land here, so only the dimmed
        // backdrop around it closes Settings.
        let sheet = mouse_area(sheet).on_press(Message::SelectSettingsTab(active));

        mouse_area(
            container(center(sheet).width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(
                        Color::from_rgba(0.0, 0.0, 0.0, if th::is_light() { 0.28 } else { 0.5 })
                            .into(),
                    ),
                    ..Default::default()
                }),
        )
        .on_press(Message::CloseSettings)
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

fn bold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::DEFAULT
    }
}

fn semibold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Semibold,
        ..iced::Font::DEFAULT
    }
}

fn medium() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Medium,
        ..iced::Font::DEFAULT
    }
}

/// The sidebar's shade: the frame's in dark themes, the lane's in light.
fn nav_background() -> Color {
    if th::is_light() {
        th::track_bg()
    } else {
        th::display_bg()
    }
}

/// The Settings sheet's size in a window `window` big: its designed size,
/// or what fits inside a margin when the window is smaller.
fn settings_sheet_size(window: iced::Size) -> iced::Size {
    iced::Size::new(
        SHEET_W.min(window.width - SHEET_MARGIN * 2.0).max(0.0),
        SHEET_H.min(window.height - SHEET_MARGIN * 2.0).max(0.0),
    )
}

/// A sidebar section: a tinted pill while it is the one shown.
fn settings_tab_style(is_active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: if is_active {
            Some(th::blend(nav_background(), th::accent(), 0.16).into())
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(th::bg_hover().into())
        } else {
            None
        },
        text_color: th::text(),
        border: iced::Border {
            radius: 7.0.into(),
            ..Default::default()
        },
        ..Default::default()
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

#[cfg(test)]
mod sheet_tests {
    use super::*;
    use iced::Size;

    #[test]
    fn the_settings_sheet_is_900_by_640_in_every_supported_window() {
        for window in [Size::new(1440.0, 900.0), Size::new(1280.0, 760.0)] {
            assert_eq!(settings_sheet_size(window), Size::new(900.0, 640.0));
        }
    }

    #[test]
    fn a_window_too_small_for_the_sheet_shrinks_it_inside_a_margin() {
        let sheet = settings_sheet_size(Size::new(800.0, 500.0));
        assert!(
            sheet.width > 0.0 && sheet.width <= 800.0 - 48.0,
            "{sheet:?}"
        );
        assert!(
            sheet.height > 0.0 && sheet.height <= 500.0 - 48.0,
            "{sheet:?}"
        );
    }

    #[test]
    fn the_selected_section_is_a_rounded_tinted_pill_without_an_outline() {
        let selected = settings_tab_style(true, button::Status::Active);
        assert_eq!(selected.border.width, 0.0);
        assert!(selected.border.radius.top_left > 0.0);
        assert!(selected.background.is_some());
        assert_eq!(selected.text_color, th::text());
    }
}
