//! Perform workspace shell, Pad Surface, and shared Section Timeline Editor.

use iced::widget::{
    button, center, column, container, horizontal_space, pick_list, row, text, text_input, tooltip,
};
use iced::{Element, Length, Theme};

use crate::domains::perform::{PerformMode, PerformMsg, Section};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

use super::*;

const MODE_SELECTOR_HEIGHT: f32 = 34.0;
const MODE_SELECTOR_INSET: f32 = 17.0;
const MODE_TAB_MIN_WIDTH: f32 = 92.0;
const MODE_TAB_MAX_WIDTH: f32 = 132.0;
const PAD_SURFACE_MIN_WIDTH: f32 = 320.0;
const SECTION_CONSTRUCTION_MIN_WIDTH: f32 = 460.0;
const SECTION_TOOLBAR_INLINE_MIN_WIDTH: f32 = 640.0;
pub(super) const SECTION_TRACK_GUTTER_WIDTH: f32 = 112.0;
pub(super) const SECTION_BAR_WIDTH: f32 = 160.0;

pub(super) fn perform_tool_button(
    icon: char,
    tooltip_label: &'static str,
    message: Message,
    active: bool,
    destructive: bool,
) -> Element<'static, Message> {
    let color = if destructive {
        th::danger()
    } else if active {
        th::accent()
    } else {
        th::text_dim()
    };
    let control = button(
        center(icons::icon(icon).size(12).color(color))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(message)
    .width(Length::Fixed(30.0))
    .height(Length::Fixed(28.0))
    .padding(0)
    .style(move |_theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if hovered {
                    th::bg_hover()
                } else if active {
                    th::bg_elevated()
                } else {
                    th::bg_surface()
                }
                .into(),
            ),
            text_color: color,
            border: iced::Border {
                color: if destructive && hovered {
                    th::danger()
                } else if active {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    });
    let hint = container(text(tooltip_label).size(10).color(th::text()))
        .padding([5, 7])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    tooltip(control, hint, tooltip::Position::Bottom)
        .gap(6)
        .padding(0)
        .into()
}

fn perform_mode_tab_width(surface_width: f32) -> f32 {
    ((surface_width - MODE_SELECTOR_INSET) / PerformMode::ALL.len() as f32)
        .clamp(MODE_TAB_MIN_WIDTH, MODE_TAB_MAX_WIDTH)
}

fn effective_perform_surface_width(preferred_width: f32, workspace_width: f32) -> f32 {
    let maximum = (workspace_width
        - SECTION_CONSTRUCTION_MIN_WIDTH
        - super::views_shell::HORIZONTAL_PANE_SPLITTER_WIDTH)
        .max(0.0);
    let minimum = PAD_SURFACE_MIN_WIDTH.min(maximum);
    preferred_width.clamp(minimum, maximum)
}

fn section_toolbar_stacks(section_width: f32) -> bool {
    section_width < SECTION_TOOLBAR_INLINE_MIN_WIDTH
}

pub(super) fn perform_pad_grid_height(window_height: f32) -> f32 {
    (window_height * 0.48).clamp(272.0, 480.0)
}

impl App {
    pub(super) fn view_perform(&self) -> Element<'_, Message> {
        let workspace_width = self.perform_workspace_width();
        let surface_width =
            effective_perform_surface_width(self.state.view.perform_surface_width, workspace_width);
        let section_width =
            (workspace_width - surface_width - super::views_shell::HORIZONTAL_PANE_SPLITTER_WIDTH)
                .max(0.0);
        let mode_selector = self.view_perform_mode_selector(surface_width);
        let pad_surface = self.view_pad_surface(surface_width);
        let section_construction = self.view_section_construction(section_width);

        let workspace = row![
            pad_surface,
            self.view_perform_surface_splitter(),
            section_construction
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0);

        container(column![mode_selector, workspace].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn perform_workspace_width(&self) -> f32 {
        let browser_width = if self.state.browser.open {
            self.state
                .browser
                .effective_dock_width(self.state.view.window_width)
                + super::views_shell::HORIZONTAL_PANE_SPLITTER_WIDTH
        } else {
            0.0
        };
        (self.state.view.window_width - browser_width).max(0.0)
    }

    pub(super) fn perform_surface_drag_width(&self, cursor_x: f32) -> f32 {
        let workspace_width = self.perform_workspace_width();
        let workspace_left = self.state.view.window_width - workspace_width;
        effective_perform_surface_width(cursor_x - workspace_left, workspace_width)
    }

    fn view_perform_mode_selector(&self, surface_width: f32) -> Element<'_, Message> {
        let tab_width = perform_mode_tab_width(surface_width);
        let mut modes = row![].height(Length::Fill).spacing(1);
        for mode in PerformMode::ALL {
            let active = self.state.perform.mode == mode;
            let text_color = if active {
                th::accent()
            } else {
                th::blend(th::text_dim(), th::text(), 0.38)
            };
            let label = mode.label().to_uppercase();
            let shortcut_color = if active {
                th::blend(th::accent_dim(), th::accent(), 0.48)
            } else {
                th::blend(th::text_dim(), th::text(), 0.18)
            };
            let shortcut = container(
                text(mode.shortcut())
                    .font(PERFORM_TECH_STRONG)
                    .size(9)
                    .color(shortcut_color),
            )
            .padding([2, 4])
            .style(move |_theme: &Theme| container::Style {
                background: Some(
                    if active {
                        th::perform_active_surface()
                    } else {
                        th::bg_dark()
                    }
                    .into(),
                ),
                border: iced::Border {
                    color: if active {
                        th::accent_dim()
                    } else {
                        th::border_light()
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            });
            let tab_content = row![
                text(label).font(PERFORM_LABEL).size(11).color(text_color),
                shortcut
            ]
            .spacing(9)
            .align_y(iced::Alignment::Center);
            let tab_button = button(center(tab_content).width(Length::Fill).height(Length::Fill))
                .on_press(Message::Perform(PerformMsg::SelectMode(mode)))
                .width(Length::Fill)
                .height(Length::Fixed(MODE_SELECTOR_HEIGHT - 2.0))
                .padding(0)
                .style(move |_theme: &Theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let background = if active {
                        th::perform_active_surface()
                    } else if hovered {
                        th::bg_hover()
                    } else {
                        iced::Color::TRANSPARENT
                    };
                    button::Style {
                        background: Some(background.into()),
                        text_color,
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                });

            let underline_color = if active {
                th::accent()
            } else {
                iced::Color::TRANSPARENT
            };
            let underline = container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fixed(2.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(underline_color.into()),
                    ..Default::default()
                });

            modes = modes.push(
                column![tab_button, underline]
                    .width(Length::Fixed(tab_width))
                    .height(Length::Fill),
            );
        }

        container(row![modes, horizontal_space()].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(MODE_SELECTOR_HEIGHT))
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: MODE_SELECTOR_INSET,
            })
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::divider(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_perform_surface_splitter(&self) -> Element<'_, Message> {
        super::views_shell::horizontal_pane_splitter(
            self.state.view.perform_surface_resize_active,
            Message::View(crate::domains::view::ViewMsg::BeginPerformSurfaceResize),
        )
    }

    pub(super) fn view_section_toolbar(
        &self,
        section: Option<&Section>,
        section_width: f32,
    ) -> Element<'_, Message> {
        let Some(section) = section else {
            return container(row![
                column![
                    text("SECTION CONSTRUCTION")
                        .font(PERFORM_LABEL)
                        .size(9)
                        .color(th::accent()),
                    text("No Section selected")
                        .font(PERFORM_DISPLAY)
                        .size(19)
                        .color(th::text())
                ]
                .spacing(4),
                horizontal_space(),
                column![
                    text("LOCAL TIMELINE")
                        .font(PERFORM_LABEL)
                        .size(9)
                        .color(th::blend(th::text_dim(), th::text(), 0.28)),
                    text("— BARS").font(PERFORM_TECH).size(9).color(th::blend(
                        th::text_dim(),
                        th::text(),
                        0.2
                    ))
                ]
                .spacing(4)
                .align_x(iced::Alignment::End)
            ])
            .align_y(iced::Alignment::Center)
            .padding([12, 16])
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into();
        };

        let bars = section.length_beats / 4.0;
        let duplicate_message = if self.state.perform.duplicate_source == Some(section.id) {
            PerformMsg::CancelDuplicateSection
        } else {
            PerformMsg::BeginDuplicateSection(section.id)
        };
        let editing_name = self.state.perform.editing_section_name == Some(section.id);
        let name: Element<'_, Message> = if editing_name {
            text_input("Section name", &self.state.perform.section_name_edit)
                .on_input(|name| Message::Perform(PerformMsg::SectionNameInput(name)))
                .on_submit(Message::Perform(PerformMsg::CommitSectionName(section.id)))
                .font(PERFORM_DISPLAY)
                .size(16)
                .padding([4, 7])
                .width(Length::Fill)
                .into()
        } else {
            button(
                text(section.name.clone())
                    .font(PERFORM_DISPLAY)
                    .size(17)
                    .color(th::text()),
            )
            .on_press(Message::Perform(PerformMsg::StartEditingSectionName(
                section.id,
            )))
            .padding([4, 6])
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
            .into()
        };
        let shorten = perform_tool_button(
            icons::MINUS,
            "Shorten Section by one bar",
            Message::Perform(PerformMsg::SetSectionLengthBeats(
                section.id,
                section.length_beats - 4.0,
            )),
            false,
            false,
        );
        let extend = perform_tool_button(
            icons::PLUS,
            "Extend Section by one bar",
            Message::Perform(PerformMsg::SetSectionLengthBeats(
                section.id,
                section.length_beats + 4.0,
            )),
            false,
            false,
        );
        let length: Element<'_, Message> = row![
            shorten,
            center(
                text(format!("{bars:.0} BARS"))
                    .font(PERFORM_TECH_STRONG)
                    .size(9)
                    .color(th::text())
            )
            .width(Length::Fixed(56.0))
            .height(Length::Fixed(28.0)),
            extend,
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .into();
        let section_id = section.id;
        let launch: Element<'_, Message> = pick_list(
            vibez_project::SectionLaunchQuantization::ALL,
            Some(section.launch_quantization),
            move |quantization| {
                Message::Perform(PerformMsg::SetSectionLaunchQuantization(
                    section_id,
                    quantization,
                ))
            },
        )
        .width(Length::Fixed(132.0))
        .padding([5, 8])
        .text_size(9)
        .style(|_theme: &Theme, status| {
            let highlighted = matches!(
                status,
                pick_list::Status::Hovered | pick_list::Status::Opened
            );
            pick_list::Style {
                text_color: th::text(),
                placeholder_color: th::text_dim(),
                handle_color: if highlighted {
                    th::accent()
                } else {
                    th::text_dim()
                },
                background: th::perform_inset().into(),
                border: iced::Border {
                    color: if highlighted {
                        th::accent_dim()
                    } else {
                        th::border_light()
                    },
                    width: 1.0,
                    radius: 1.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 1.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into();
        let loop_toggle = perform_tool_button(
            icons::REPEAT,
            "Toggle Section loop",
            Message::Perform(PerformMsg::ToggleSectionLoop(section.id)),
            section.looping,
            false,
        );
        let duplicate = perform_tool_button(
            if self.state.perform.duplicate_source == Some(section.id) {
                icons::X
            } else {
                icons::COPY
            },
            if self.state.perform.duplicate_source == Some(section.id) {
                "Cancel Section duplication"
            } else {
                "Duplicate Section"
            },
            Message::Perform(duplicate_message),
            self.state.perform.duplicate_source == Some(section.id),
            false,
        );
        let expand = perform_tool_button(
            icons::LAYOUT_LIST,
            if self.state.perform.section_timeline_expanded {
                "Compact Section timeline"
            } else {
                "Expand Section timeline"
            },
            Message::Perform(PerformMsg::ToggleSectionTimelineExpanded),
            self.state.perform.section_timeline_expanded,
            false,
        );
        let delete = perform_tool_button(
            icons::TRASH_2,
            "Delete Section",
            Message::Perform(PerformMsg::DeleteSection(section.id)),
            false,
            true,
        );
        let divider = || {
            container(horizontal_space())
                .width(Length::Fixed(1.0))
                .height(Length::Fixed(22.0))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::divider().into()),
                    ..Default::default()
                })
        };
        let controls = container(
            row![
                length,
                divider(),
                launch,
                divider(),
                loop_toggle,
                divider(),
                expand,
                divider(),
                duplicate,
                divider(),
                delete,
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        )
        .padding(3)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::perform_inset().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
        let identity = column![
            text(format!("SELECTED SECTION · {:02}", section.slot + 1))
                .font(PERFORM_LABEL)
                .size(7)
                .color(th::text_dim()),
            name
        ]
        .spacing(3)
        .width(Length::Fill);
        let toolbar: Element<'_, Message> = if section_toolbar_stacks(section_width) {
            column![identity, controls]
                .spacing(8)
                .padding([8, 12])
                .into()
        } else {
            row![identity, controls]
                .spacing(12)
                .align_y(iced::Alignment::Center)
                .padding([8, 12])
                .into()
        };

        container(toolbar)
            .width(Length::Fill)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_tabs_scale_with_the_pad_surface_then_stop_growing() {
        let narrow = perform_mode_tab_width(320.0);
        let default = perform_mode_tab_width(560.0);
        let wide = perform_mode_tab_width(960.0);

        assert!((MODE_TAB_MIN_WIDTH..MODE_TAB_MAX_WIDTH).contains(&narrow));
        assert_eq!(default, MODE_TAB_MAX_WIDTH);
        assert_eq!(wide, MODE_TAB_MAX_WIDTH);
        assert!(MODE_SELECTOR_INSET + narrow * 3.0 <= 320.0);
    }

    #[test]
    fn pad_grid_height_responds_only_to_window_geometry() {
        assert_eq!(perform_pad_grid_height(400.0), 272.0);
        assert_eq!(perform_pad_grid_height(900.0), 432.0);
        assert_eq!(perform_pad_grid_height(1400.0), 480.0);
    }

    #[test]
    fn perform_surface_width_preserves_both_panes_during_drag() {
        assert_eq!(effective_perform_surface_width(200.0, 1400.0), 320.0);
        assert_eq!(effective_perform_surface_width(640.0, 1400.0), 640.0);
        assert_eq!(effective_perform_surface_width(1200.0, 1400.0), 933.0);
        assert_eq!(effective_perform_surface_width(500.0, 820.0), 353.0);
    }

    #[test]
    fn section_toolbar_stacks_before_identity_text_collapses() {
        assert!(section_toolbar_stacks(SECTION_CONSTRUCTION_MIN_WIDTH));
        assert!(!section_toolbar_stacks(760.0));
    }
}
