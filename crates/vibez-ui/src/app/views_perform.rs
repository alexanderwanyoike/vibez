//! Perform workspace shell: mode selector, 4x4 Pad Surface, and the empty
//! Section construction area. Musical behavior arrives in later cards.

use iced::widget::{button, center, column, container, horizontal_space, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::domains::perform::{PadPosition, PerformEditorFocus, PerformMode, PerformMsg};
use crate::message::Message;
use crate::theme as th;

use super::*;

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
        let mut modes = row![].width(Length::Fill).height(Length::Fixed(38.0));
        for mode in PerformMode::ALL {
            let active = self.state.perform.mode == mode;
            let text_color = if active { th::accent() } else { th::text_dim() };
            let tab = button(
                row![
                    text(mode.label()).size(11).color(text_color),
                    horizontal_space(),
                    text(mode.shortcut()).size(9).color(if active {
                        th::accent_dim()
                    } else {
                        th::text_muted()
                    })
                ]
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::Perform(PerformMsg::SelectMode(mode)))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .padding([0, 14])
            .style(move |_theme: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let background = if active {
                    th::bg_elevated()
                } else if hovered {
                    th::bg_hover()
                } else {
                    th::bg_surface()
                };
                button::Style {
                    background: Some(background.into()),
                    text_color,
                    border: iced::Border {
                        color: if active { th::accent() } else { th::divider() },
                        width: if active { 2.0 } else { 1.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            });
            modes = modes.push(tab);
        }

        modes.into()
    }

    fn view_pad_surface(&self) -> Element<'_, Message> {
        let mode = self.state.perform.mode;
        let heading = column![
            text("PAD SURFACE").size(9).color(th::text_dim()),
            text(mode.label()).size(19).color(th::text())
        ]
        .spacing(4);
        let origin = match mode {
            PerformMode::Sections => "ORDER · TOP-LEFT",
            PerformMode::TrackMutes => "PROJECT TRACKS · TOP-LEFT",
            PerformMode::Instrument => "ORDER · BOTTOM-LEFT",
        };
        let header = row![
            heading,
            horizontal_space(),
            text(origin).size(8).color(th::text_muted())
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
                pad_row = pad_row.push(self.view_empty_pad(position, mode));
            }
            grid = grid.push(pad_row);
        }

        let surface = container(column![header, grid].spacing(12))
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .padding(14)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
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

    fn view_empty_pad(&self, position: PadPosition, mode: PerformMode) -> Element<'_, Message> {
        let ordinal = u16::from(position.ordinal(mode))
            + u16::from(self.state.perform.banks.for_mode(mode)) * 16;
        let selected = self.state.perform.selected_pad == Some(position);
        let background = if selected {
            th::bg_hover()
        } else {
            th::bg_elevated()
        };
        let (title, detail, color) = match mode {
            PerformMode::Sections => ("+ Section".to_string(), "EMPTY", th::text_dim()),
            PerformMode::TrackMutes => {
                if let Some(track) = self
                    .state
                    .project_tracks
                    .tracks
                    .get(usize::from(ordinal - 1))
                {
                    (
                        track.name.clone(),
                        if track.kind.is_midi() {
                            "MIDI TRACK"
                        } else {
                            "AUDIO TRACK"
                        },
                        th::track_color(track.color_index),
                    )
                } else {
                    ("—".to_string(), "NO PROJECT TRACK", th::text_muted())
                }
            }
            PerformMode::Instrument => (
                "Select MIDI".to_string(),
                "NO INSTRUMENT TARGET",
                th::text_muted(),
            ),
        };

        container(
            column![
                row![
                    text(format!("{ordinal:02}")).size(10).color(color),
                    horizontal_space(),
                    text(format!("R{}C{}", position.row + 1, position.column + 1))
                        .size(7)
                        .color(th::text_muted())
                ],
                center(
                    text(title)
                        .size(12)
                        .color(if mode == PerformMode::Sections {
                            th::text_dim()
                        } else {
                            th::text()
                        })
                )
                .width(Length::Fill)
                .height(Length::Fill),
                text(detail).size(7).color(th::text_muted())
            ]
            .height(Length::Fill),
        )
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .padding(9)
        .style(move |_theme: &Theme| container::Style {
            background: Some(background.into()),
            border: iced::Border {
                color: if selected {
                    th::accent()
                } else if mode == PerformMode::Sections {
                    th::border_light()
                } else {
                    color
                },
                width: 1.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        })
        .into()
    }

    fn view_section_construction(&self) -> Element<'_, Message> {
        let toolbar = row![
            column![
                text("SECTION CONSTRUCTION").size(9).color(th::text_dim()),
                text("No Section selected").size(18).color(th::text())
            ]
            .spacing(4),
            horizontal_space(),
            column![
                text("LOCAL TIMELINE").size(8).color(th::text_muted()),
                text("— BARS").size(10).color(th::text_dim())
            ]
            .spacing(4)
            .align_x(iced::Alignment::End)
        ]
        .align_y(iced::Alignment::Center)
        .padding([12, 16]);

        let ruler = row![
            container(text("PROJECT TRACK").size(7).color(th::text_muted()))
                .width(Length::Fixed(112.0))
                .padding([8, 10]),
            container(text("1").size(8).color(th::text_muted()))
                .width(Length::FillPortion(1))
                .padding(8),
            container(text("2").size(8).color(th::text_muted()))
                .width(Length::FillPortion(1))
                .padding(8),
            container(text("3").size(8).color(th::text_muted()))
                .width(Length::FillPortion(1))
                .padding(8),
            container(text("4").size(8).color(th::text_muted()))
                .width(Length::FillPortion(1))
                .padding(8)
        ]
        .height(Length::Fixed(30.0));

        let empty = center(
            column![
                text("SECTION SPACE").size(9).color(th::accent_dim()),
                text("Select a Section to construct its multitrack timeline")
                    .size(14)
                    .color(th::text_dim()),
                text("Section creation and editing are not available yet")
                    .size(9)
                    .color(th::text_muted())
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        let construction = container(column![toolbar, ruler, empty].height(Length::Fill))
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
