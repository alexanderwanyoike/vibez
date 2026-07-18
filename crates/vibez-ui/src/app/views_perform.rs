//! Perform workspace shell: mode selector, 4x4 Pad Surface, and the empty
//! Section construction area. Musical behavior arrives in later cards.

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, pick_list, row, scrollable,
    stack, text, text_input, tooltip,
};
use iced::{Element, Length, Shadow, Theme, Vector};

use crate::domains::perform::{PadPosition, PerformEditorFocus, PerformMode, PerformMsg, Section};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

use super::*;

const MODE_SELECTOR_HEIGHT: f32 = 34.0;
const MODE_SELECTOR_INSET: f32 = 17.0;
const MODE_TAB_MIN_WIDTH: f32 = 108.0;
const MODE_TAB_MAX_WIDTH: f32 = 132.0;
const PAD_SURFACE_WIDTH_SHARE: f32 = 0.4;
const SECTION_TRACK_GUTTER_WIDTH: f32 = 112.0;
const SECTION_BAR_WIDTH: f32 = 160.0;

fn perform_tool_button(
    icon: char,
    help: impl Into<String>,
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
    tooltip(
        control,
        text(help.into())
            .font(PERFORM_TECH)
            .size(9)
            .color(th::text()),
        tooltip::Position::Bottom,
    )
    .gap(6)
    .padding(6)
    .style(|_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            color: th::border_light(),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn perform_mode_tab_width(window_width: f32) -> f32 {
    ((window_width * PAD_SURFACE_WIDTH_SHARE - MODE_SELECTOR_INSET) / PerformMode::ALL.len() as f32)
        .clamp(MODE_TAB_MIN_WIDTH, MODE_TAB_MAX_WIDTH)
}

impl App {
    pub(super) fn view_perform(&self) -> Element<'_, Message> {
        let mode_selector = self.view_perform_mode_selector();
        let pad_surface = self.view_pad_surface();
        let section_construction = self.view_section_construction();

        let workspace = row![pad_surface, section_construction]
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

    fn view_perform_mode_selector(&self) -> Element<'_, Message> {
        let tab_width = perform_mode_tab_width(self.state.view.window_width);
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

    fn view_pad_surface(&self) -> Element<'_, Message> {
        let mode = self.state.perform.mode;
        let heading = column![
            text("PERFORM SURFACE")
                .font(PERFORM_LABEL)
                .size(9)
                .color(th::accent()),
            text(mode.label().to_uppercase())
                .font(PERFORM_DISPLAY)
                .size(22)
                .color(th::text())
        ]
        .spacing(5);
        let origin = match mode {
            PerformMode::Sections => "ORDER · TOP-LEFT".to_string(),
            PerformMode::TrackMutes => format!(
                "BANK {} · PROJECT TRACKS · [ ]",
                self.state.perform.banks.track_mutes + 1
            ),
            PerformMode::Instrument => "ORDER · BOTTOM-LEFT".to_string(),
        };
        let header = row![
            heading,
            horizontal_space(),
            text(origin).font(PERFORM_TECH).size(9).color(th::blend(
                th::text_dim(),
                th::text(),
                0.24
            ))
        ]
        .align_y(iced::Alignment::End);

        let mut grid = column![]
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(8);
        for row_index in 0..4 {
            let mut pad_row = row![]
                .width(Length::Fill)
                .height(Length::FillPortion(1))
                .spacing(8);
            for position in PadPosition::ALL
                .iter()
                .copied()
                .filter(|position| position.row == row_index)
            {
                pad_row = pad_row.push(self.view_perform_pad(position, mode));
            }
            grid = grid.push(pad_row);
        }
        let grid = center(grid).width(Length::Fill).height(Length::Fill);

        let surface = container(column![header, grid].spacing(12))
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .padding(14)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::perform_inset().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

        mouse_area(surface)
            .on_press(Message::Perform(PerformMsg::FocusEditor(
                PerformEditorFocus::PadSurface,
            )))
            .into()
    }

    fn view_perform_pad(&self, position: PadPosition, mode: PerformMode) -> Element<'_, Message> {
        let ordinal = u16::from(position.ordinal(mode))
            + u16::from(self.state.perform.banks.for_mode(mode)) * 16;
        let section = (mode == PerformMode::Sections)
            .then(|| self.state.perform.sections.at_slot(ordinal - 1))
            .flatten();
        let selected = section
            .is_some_and(|section| self.state.perform.selected_section == Some(section.id))
            || self.state.perform.selected_pad == Some(position);
        let pressed = self.state.perform.is_pad_pressed(position);
        let mute_track = (mode == PerformMode::TrackMutes)
            .then(|| {
                self.state
                    .perform
                    .track_for_mute_pad(position, &self.state.project_tracks.tracks)
            })
            .flatten();
        let (title, detail, color, muted) = match mode {
            PerformMode::Sections => match section {
                Some(section) => (
                    section.name.clone(),
                    format!("AVAILABLE · {:.0} BARS", section.length_beats / 4.0),
                    th::track_color((ordinal - 1) as u8),
                    false,
                ),
                None if self.state.perform.duplicate_source.is_some() => (
                    "+ DUPLICATE".to_string(),
                    "CHOOSE EMPTY SLOT".to_string(),
                    th::track_color((ordinal - 1) as u8),
                    false,
                ),
                None => (
                    "+ SECTION".to_string(),
                    "EMPTY".to_string(),
                    th::track_color((ordinal - 1) as u8),
                    false,
                ),
            },
            PerformMode::TrackMutes => {
                if let Some(track) = mute_track {
                    (
                        track.name.clone(),
                        format!(
                            "{} · {}",
                            if track.kind.is_midi() {
                                "MIDI"
                            } else {
                                "AUDIO"
                            },
                            if track.mute { "MUTED" } else { "LIVE" }
                        ),
                        th::track_color(track.color_index),
                        track.mute,
                    )
                } else {
                    (
                        "—".to_string(),
                        "NO PROJECT TRACK".to_string(),
                        th::text_muted(),
                        false,
                    )
                }
            }
            PerformMode::Instrument => (
                "SELECT MIDI".to_string(),
                "NO INSTRUMENT TARGET".to_string(),
                th::track_color((ordinal - 1) as u8),
                false,
            ),
        };
        let number_color = th::blend(color, th::text(), 0.3);
        let coordinate_color = th::blend(th::text_dim(), th::text(), 0.2);

        let pad_face = container(
            column![
                row![
                    text(format!("{ordinal:02}"))
                        .font(PERFORM_TECH_STRONG)
                        .size(11)
                        .color(number_color),
                    horizontal_space(),
                    text(format!("R{} · C{}", position.row + 1, position.column + 1))
                        .font(PERFORM_TECH)
                        .size(9)
                        .color(coordinate_color)
                ],
                center(text(title).font(PERFORM_DISPLAY).size(13).color(
                    if mode == PerformMode::Sections {
                        th::blend(th::text_dim(), th::text(), 0.22)
                    } else {
                        th::text()
                    }
                ))
                .width(Length::Fill)
                .height(Length::Fill),
                text(detail).font(PERFORM_TECH).size(8).color(th::blend(
                    th::text_dim(),
                    th::text(),
                    0.12
                ))
            ]
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8)
        .style(move |_theme: &Theme| container::Style {
            background: Some(
                iced::gradient::Linear::new(2.35)
                    .add_stop(
                        0.0,
                        if pressed {
                            th::blend(th::accent_dim(), color, 0.35)
                        } else if muted {
                            th::blend(th::mute_active(), color, 0.28)
                        } else if selected {
                            th::bg_hover()
                        } else {
                            th::perform_pad_highlight()
                        },
                    )
                    .add_stop(1.0, th::perform_pad_lowlight())
                    .into(),
            ),
            border: iced::Border {
                color: if pressed || selected {
                    th::accent()
                } else if muted {
                    th::mute_active()
                } else {
                    th::blend(th::border_light(), color, 0.38)
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        });

        let pad: Element<'_, Message> = container(pad_face)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .padding(3)
            .style(move |_theme: &Theme| container::Style {
                background: Some(
                    if pressed {
                        th::blend(th::bg_dark(), color, 0.28)
                    } else {
                        th::bg_dark()
                    }
                    .into(),
                ),
                border: iced::Border {
                    color: th::blend(th::border(), color, 0.3),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: Shadow {
                    color: th::perform_shadow(),
                    offset: Vector::new(0.0, if pressed { 1.0 } else { 3.0 }),
                    blur_radius: if pressed { 3.0 } else { 7.0 },
                },
                ..Default::default()
            })
            .into();

        match (mode, section) {
            (PerformMode::Sections, Some(section)) => mouse_area(pad)
                .on_press(Message::Perform(PerformMsg::SelectSection(section.id)))
                .into(),
            (PerformMode::Sections, None) if self.state.perform.duplicate_source.is_some() => {
                mouse_area(pad)
                    .on_press(Message::Perform(PerformMsg::DuplicateSectionTo(
                        ordinal - 1,
                    )))
                    .into()
            }
            (PerformMode::Sections, None) => mouse_area(pad)
                .on_press(Message::Perform(PerformMsg::CreateSectionAt(ordinal - 1)))
                .into(),
            (PerformMode::TrackMutes, _) if mute_track.is_some() => mouse_area(pad)
                .on_press(Message::Perform(PerformMsg::ToggleTrackMuteFromPad(
                    position,
                )))
                .into(),
            _ => pad,
        }
    }

    fn view_section_toolbar(&self, section: Option<&Section>) -> Element<'_, Message> {
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
            "Shorten Section by 1 bar",
            Message::Perform(PerformMsg::SetSectionLengthBeats(
                section.id,
                section.length_beats - 4.0,
            )),
            false,
            false,
        );
        let extend = perform_tool_button(
            icons::PLUS,
            "Extend Section by 1 bar",
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
            if section.looping {
                "Disable Section looping"
            } else {
                "Enable Section looping"
            },
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
                "Cancel duplicate"
            } else {
                "Duplicate Section into an empty pad"
            },
            Message::Perform(duplicate_message),
            self.state.perform.duplicate_source == Some(section.id),
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
        let toolbar = row![identity, controls]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .padding([8, 12]);

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

    fn view_section_construction(&self) -> Element<'_, Message> {
        let selected = self
            .state
            .perform
            .selected_section
            .and_then(|id| self.state.perform.sections.by_id(id));
        let toolbar = self.view_section_toolbar(selected);

        let bar_count = selected
            .map(|section| (section.length_beats / 4.0).round() as usize)
            .unwrap_or(4)
            .max(1);
        let timeline_width = SECTION_BAR_WIDTH * bar_count as f32;
        let ruler_number_color = th::blend(th::text_dim(), th::text(), 0.36);
        let ruler_gutter = container(
            text("PROJECT TRACK")
                .font(PERFORM_TECH)
                .size(8)
                .color(th::blend(th::text_dim(), th::text(), 0.16)),
        )
        .width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH))
        .height(Length::Fixed(30.0))
        .padding([8, 10])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::perform_grid_line(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });
        let mut ruler_marks = row![]
            .width(Length::Fixed(timeline_width))
            .height(Length::Fixed(30.0));
        for bar in 0..bar_count {
            ruler_marks = ruler_marks.push(
                container(
                    text((bar + 1).to_string())
                        .font(PERFORM_TECH_STRONG)
                        .size(11)
                        .color(ruler_number_color),
                )
                .width(Length::Fixed(SECTION_BAR_WIDTH))
                .height(Length::Fixed(30.0))
                .padding(8)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::perform_grid_line(),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }

        let mut track_gutters = column![].width(Length::Fill).height(Length::Fill);
        let mut timeline_tracks = column![]
            .width(Length::Fixed(timeline_width))
            .height(Length::Fill);
        for index in 0..6_u8 {
            let marker = container(horizontal_space())
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(18.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(th::with_alpha(th::track_color(index), 0.38).into()),
                    ..Default::default()
                });
            let gutter =
                container(row![marker, horizontal_space()].align_y(iced::Alignment::Center))
                    .width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH))
                    .height(Length::Fill)
                    .padding([0, 10])
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::bg_surface().into()),
                        border: iced::Border {
                            color: th::perform_grid_line(),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    });
            let mut lanes = row![]
                .width(Length::Fixed(timeline_width))
                .height(Length::Fill);
            for _ in 0..bar_count {
                lanes = lanes.push(
                    container(horizontal_space())
                        .width(Length::Fixed(SECTION_BAR_WIDTH))
                        .height(Length::Fill)
                        .style(|_theme: &Theme| container::Style {
                            border: iced::Border {
                                color: th::perform_grid_line(),
                                width: 1.0,
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        }),
                );
            }
            track_gutters = track_gutters.push(gutter.height(Length::FillPortion(1)));
            timeline_tracks = timeline_tracks.push(lanes.height(Length::FillPortion(1)));
        }
        let fixed_gutter = column![ruler_gutter, track_gutters]
            .width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH))
            .height(Length::Fill);
        let timeline_content =
            container(column![ruler_marks, timeline_tracks].height(Length::Fill))
                .width(Length::Fixed(timeline_width))
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::display_bg().into()),
                    ..Default::default()
                });
        let scrolling_timeline = scrollable::Scrollable::with_direction(
            timeline_content,
            scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new()
                    .width(5)
                    .scroller_width(5)
                    .spacing(1),
            ),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let timeline_grid = row![fixed_gutter, scrolling_timeline]
            .width(Length::Fill)
            .height(Length::Fill);

        let (empty_title, empty_detail, empty_footnote) = if selected.is_some() {
            (
                format!("{bar_count} BAR SECTION"),
                "This Section has its own local timeline",
                "MUSICAL AUTHORING ARRIVES IN CARD 07",
            )
        } else {
            (
                "SELECT A SECTION".to_string(),
                "Create one from an empty Pad Position",
                "SECTION DATA IS SAVED WITH THE PROJECT",
            )
        };
        let empty = center(
            container(
                column![
                    text("SECTION SPACE")
                        .font(PERFORM_LABEL)
                        .size(9)
                        .color(th::accent()),
                    text(empty_title)
                        .font(PERFORM_LABEL)
                        .size(13)
                        .color(th::text()),
                    text(empty_detail).size(10).color(th::text_dim()),
                    text(empty_footnote)
                        .font(PERFORM_TECH)
                        .size(8)
                        .color(th::blend(th::text_dim(), th::text(), 0.1))
                ]
                .spacing(8)
                .align_x(iced::Alignment::Center),
            )
            .padding([16, 22])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::perform_inset().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                shadow: Shadow {
                    color: th::perform_shadow(),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 10.0,
                },
                ..Default::default()
            }),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let empty_timeline = stack![timeline_grid, empty];

        let construction = container(column![toolbar, empty_timeline].height(Length::Fill))
            .width(Length::FillPortion(3))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

        mouse_area(construction)
            .on_press(Message::Perform(PerformMsg::FocusEditor(
                PerformEditorFocus::SectionConstruction,
            )))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_tabs_scale_with_the_pad_surface_then_stop_growing() {
        let narrow = perform_mode_tab_width(900.0);
        let default = perform_mode_tab_width(1400.0);
        let wide = perform_mode_tab_width(2000.0);

        assert!((MODE_TAB_MIN_WIDTH..MODE_TAB_MAX_WIDTH).contains(&narrow));
        assert_eq!(default, MODE_TAB_MAX_WIDTH);
        assert_eq!(wide, MODE_TAB_MAX_WIDTH);
        assert!(MODE_SELECTOR_INSET + narrow * 3.0 <= 900.0 * PAD_SURFACE_WIDTH_SHARE);
    }
}
