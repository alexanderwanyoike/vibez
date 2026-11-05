//! The Settings sheet: a section sidebar beside the chosen section, plus
//! the Plugins and Warping sections.

use iced::widget::{button, center, column, container, mouse_area, row, scrollable, text};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::SettingsTab;
use crate::theme as th;

use super::views_settings_rows::{action, group, hint, setting_row, slider_control, switch};
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
                hint(section.hint),
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
        let settings = &self.state.plugin_settings;
        let mut folders: Vec<Element<'_, Message>> = vec![
            setting_row(
                "Scan default system paths",
                Some(hint("The standard VST3 and CLAP folders for this system.").into()),
                switch(settings.scan_default_paths, Message::ToggleScanDefaultPaths),
            ),
            setting_row(
                "Extra folders",
                Some(
                    hint(if settings.extra_scan_paths.is_empty() {
                        "None yet. Add a folder to scan it as well."
                    } else {
                        "Scanned as well as the default paths."
                    })
                    .into(),
                ),
                action("Add folder", Some(Message::AddPluginScanPath), false),
            ),
        ];
        for (index, path) in settings.extra_scan_paths.iter().enumerate() {
            folders.push(
                row![
                    icons::icon(icons::FOLDER).size(13).color(th::text_dim()),
                    text(path.display().to_string())
                        .size(11.5)
                        .color(th::text_dim())
                        .width(Length::Fill),
                    action("Remove", Some(Message::RemovePluginScanPath(index)), false),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into(),
            );
        }
        let status = if self.state.plugin_scan_status.is_empty() {
            format!("{} plugins cached", settings.cache.len())
        } else {
            self.state.plugin_scan_status.clone()
        };
        let scan_label = if self.state.plugin_scan_in_progress {
            "Scanning…"
        } else {
            "Scan plugins"
        };
        column![
            group(folders),
            group([setting_row(
                "Plugin library",
                Some(hint(status).into()),
                action(scan_label, Some(Message::ScanPlugins), true),
            )]),
        ]
        .spacing(16)
        .into()
    }

    pub(super) fn view_settings_warping_tab(&self) -> Element<'_, Message> {
        let confidence = self.state.warp_confidence_threshold;
        group([
            setting_row(
                "Auto-warp samples on import",
                Some(
                    hint(
                        "Detects each dropped sample's tempo and stretches it to the \
                         project's, keeping its pitch.",
                    )
                    .into(),
                ),
                switch(
                    self.state.auto_warp_on_import,
                    Message::ToggleAutoWarpOnImport,
                ),
            ),
            setting_row(
                "Detection confidence",
                Some(
                    hint("Higher warps only when the detector is sure; lower warps ambiguous clips too.")
                        .into(),
                ),
                slider_control(
                    0.0..=1.0,
                    confidence,
                    0.05,
                    Message::SetWarpConfidenceThreshold,
                    format!("{confidence:.2}"),
                ),
            ),
            setting_row(
                "Re-warp all clips",
                Some(hint("Stretches every warped clip to the current project tempo.").into()),
                action("Re-warp", Some(Message::RewarpAllClips), false),
            ),
        ])
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
