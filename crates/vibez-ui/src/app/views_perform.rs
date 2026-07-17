//! Perform workspace shell: mode selector, 4x4 Pad Surface, and the empty
//! Section construction area. Musical behavior arrives in later cards.

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, stack, text,
};
use iced::{Element, Length, Shadow, Theme, Vector};

use crate::domains::perform::{PadPosition, PerformEditorFocus, PerformMode, PerformMsg};
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

use super::*;

const MODE_SELECTOR_HEIGHT: f32 = 34.0;
const MODE_SELECTOR_INSET: f32 = 17.0;
const MODE_TAB_MIN_WIDTH: f32 = 108.0;
const MODE_TAB_MAX_WIDTH: f32 = 132.0;
const PAD_SURFACE_WIDTH_SHARE: f32 = 0.4;
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
            PerformMode::Sections => "ORDER · TOP-LEFT",
            PerformMode::TrackMutes => "PROJECT TRACKS · TOP-LEFT",
            PerformMode::Instrument => "ORDER · BOTTOM-LEFT",
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
                pad_row = pad_row.push(self.view_empty_pad(position, mode));
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

    fn view_empty_pad(&self, position: PadPosition, mode: PerformMode) -> Element<'_, Message> {
        let ordinal = u16::from(position.ordinal(mode))
            + u16::from(self.state.perform.banks.for_mode(mode)) * 16;
        let selected = self.state.perform.selected_pad == Some(position);
        let pressed = self.state.perform.is_pad_pressed(position);
        let (title, detail, color) = match mode {
            PerformMode::Sections => (
                "+ SECTION".to_string(),
                "EMPTY",
                th::track_color((ordinal - 1) as u8),
            ),
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
                "SELECT MIDI".to_string(),
                "NO INSTRUMENT TARGET",
                th::track_color((ordinal - 1) as u8),
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
                } else {
                    th::blend(th::border_light(), color, 0.38)
                },
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        });

        container(pad_face)
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
            .into()
    }

    fn view_section_construction(&self) -> Element<'_, Message> {
        let toolbar = row![
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
        ]
        .align_y(iced::Alignment::Center)
        .padding([12, 16]);
        let toolbar = container(toolbar)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

        let ruler_number_color = th::blend(th::text_dim(), th::text(), 0.36);
        let ruler = row![
            container(
                text("PROJECT TRACK")
                    .font(PERFORM_TECH)
                    .size(8)
                    .color(th::blend(th::text_dim(), th::text(), 0.16))
            )
            .width(Length::Fixed(112.0))
            .padding([8, 10]),
            container(
                text("1")
                    .font(PERFORM_TECH_STRONG)
                    .size(11)
                    .color(ruler_number_color)
            )
            .width(Length::FillPortion(1))
            .padding(8),
            container(
                text("2")
                    .font(PERFORM_TECH_STRONG)
                    .size(11)
                    .color(ruler_number_color)
            )
            .width(Length::FillPortion(1))
            .padding(8),
            container(
                text("3")
                    .font(PERFORM_TECH_STRONG)
                    .size(11)
                    .color(ruler_number_color)
            )
            .width(Length::FillPortion(1))
            .padding(8),
            container(
                text("4")
                    .font(PERFORM_TECH_STRONG)
                    .size(11)
                    .color(ruler_number_color)
            )
            .width(Length::FillPortion(1))
            .padding(8)
        ]
        .height(Length::Fixed(30.0));

        let mut ghost_tracks = column![].width(Length::Fill).height(Length::Fill);
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
                    .width(Length::Fixed(112.0))
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
            let mut lanes = row![].width(Length::Fill).height(Length::Fill);
            for _ in 0..4 {
                lanes = lanes.push(
                    container(horizontal_space())
                        .width(Length::FillPortion(1))
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
            ghost_tracks = ghost_tracks.push(
                row![gutter, lanes]
                    .width(Length::Fill)
                    .height(Length::FillPortion(1)),
            );
        }
        let timeline_grid = container(ghost_tracks)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::display_bg().into()),
                ..Default::default()
            });

        let empty = center(
            container(
                column![
                    text("SECTION SPACE")
                        .font(PERFORM_LABEL)
                        .size(9)
                        .color(th::accent()),
                    text("SELECT A SECTION")
                        .font(PERFORM_LABEL)
                        .size(13)
                        .color(th::text()),
                    text("Its multitrack timeline will open here")
                        .size(10)
                        .color(th::text_dim()),
                    text("CREATION + EDITING ARRIVE IN A LATER CARD")
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

        let construction = container(column![toolbar, ruler, empty_timeline].height(Length::Fill))
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
