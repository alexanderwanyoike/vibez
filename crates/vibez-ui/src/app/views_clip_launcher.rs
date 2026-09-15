//! Clip Project creation and grid authoring use the shared Vibez shell.

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, row, scrollable, text,
};
use iced::{Element, Length, Theme};
use vibez_project::PerformLayout;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::perform::{ClipMsg, PadPosition, PerformMode, PerformMsg};
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH};

use super::App;

fn surface(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn choice_style(selected: bool, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if selected {
                th::perform_active_surface()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: if selected { th::accent() } else { th::border() },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

impl App {
    pub(super) fn view_new_project_overlay(&self) -> Element<'_, Message> {
        let selected = self.state.project.new_project_layout.unwrap_or_default();
        let choice = |layout: PerformLayout, subtitle: &'static str, detail: &'static str| {
            let active = layout == selected;
            let mut preview = column![].spacing(4);
            for r in 0..4 {
                let mut line = row![].spacing(4);
                for c in 0..4 {
                    let accent = if layout == PerformLayout::Sections {
                        r == 0 && c == 0
                    } else {
                        r == c
                    };
                    line = line.push(
                        container(text(if accent { "•" } else { " " }).size(12))
                            .width(Length::Fill)
                            .height(22)
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(
                                    if accent {
                                        th::accent_dim()
                                    } else {
                                        th::bg_dark()
                                    }
                                    .into(),
                                ),
                                text_color: Some(th::accent()),
                                border: iced::Border {
                                    radius: 2.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    );
                }
                preview = preview.push(line);
            }
            button(
                column![
                    row![
                        text(layout.label()).font(PERFORM_DISPLAY).size(19),
                        horizontal_space(),
                        text(if active { "SELECTED" } else { "" })
                            .font(PERFORM_TECH)
                            .size(9)
                            .color(th::accent())
                    ],
                    text(subtitle).size(12).color(th::text_dim()),
                    preview,
                    text(detail).size(11).color(th::text_dim()),
                ]
                .spacing(12),
            )
            .padding(16)
            .width(Length::Fill)
            .on_press(Message::SelectNewProjectLayout(layout))
            .style(move |_theme: &Theme, status| choice_style(active, status))
        };
        let choices = row![
            choice(
                PerformLayout::Sections,
                "Play complete musical sections",
                "Build multitrack loops and launch them from pads."
            ),
            choice(
                PerformLayout::Clips,
                "Combine individual parts freely",
                "Launch Audio and MIDI Clips from a keyboard grid."
            ),
        ]
        .spacing(12)
        .width(Length::Fill);
        let create = button(text("Create Project").size(12))
            .on_press(Message::ConfirmNewProject)
            .padding([9, 18])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::accent().into()),
                text_color: th::bg_dark(),
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let card = container(column![
            text("NEW PROJECT").font(PERFORM_TECH).size(10).color(th::accent()),
            text("How do you want to perform?").font(PERFORM_DISPLAY).size(23).color(th::text()),
            choices,
            text("Your Perform layout stays with this project. Both layouts share Arrange and Mix.").size(11).color(th::text_dim()),
            row![horizontal_space(), button(text("Cancel").size(12)).on_press(Message::CancelNewProject).padding([9, 14]).style(|_theme, status| choice_style(false, status)), create].spacing(8),
        ].spacing(16)).padding(24).max_width(620).style(surface);
        container(center(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_clip_perform(&self) -> Element<'_, Message> {
        let width = self.perform_workspace_width();
        let compact = self.state.view.window_height < 800.0;
        let slot_height = if compact {
            ((self.state.view.window_height - 480.0) / 5.0).clamp(26.0, 58.0)
        } else {
            58.0
        };
        let mut workspace = column![self.view_perform_mode_selector(width)].spacing(0);
        if self.state.perform.mode != PerformMode::Sections {
            return container(workspace.push(self.view_pad_surface(width)))
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(surface)
                .into();
        }
        let move_window = |label: &'static str, tracks, rows| {
            button(text(label).font(PERFORM_TECH).size(12))
                .on_press(Message::Perform(PerformMsg::Clips(ClipMsg::MoveWindow {
                    tracks,
                    rows,
                })))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
        };
        let toolbar = row![
            text("CLIP GRID")
                .font(PERFORM_LABEL)
                .size(11)
                .color(th::text()),
            text("AUTHORING")
                .font(PERFORM_TECH)
                .size(9)
                .color(th::text_dim()),
            horizontal_space(),
            button(text("+ Audio").size(11))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
                .on_press(Message::Arrangement(ArrangementMsg::AddTrack)),
            button(text("+ MIDI").size(11))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
                .on_press(Message::Arrangement(ArrangementMsg::AddMidiTrack)),
            move_window("←", -4, 0),
            move_window("→", 4, 0),
            move_window("↑", 0, -4),
            move_window("↓", 0, 4),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        workspace = workspace.push(container(toolbar).padding([if compact { 4 } else { 10 }, 12]));
        if self.state.project_tracks.tracks.is_empty() {
            workspace = workspace.push(
                center(
                    column![
                        text("Start with a part")
                            .font(PERFORM_DISPLAY)
                            .size(22)
                            .color(th::text()),
                        text(
                            "Add an Audio or MIDI track. Each column holds its clip alternatives."
                        )
                        .size(12)
                        .color(th::text_dim()),
                    ]
                    .spacing(10),
                )
                .height(Length::Fill),
            );
        } else {
            let first_track = self.state.perform.clip_editor.first_track;
            let first_row = self.state.perform.clip_editor.first_row;
            let visible_tracks = ((width - 54.0) / 145.0).floor().max(4.0) as usize;
            let lane_width = ((width - 29.0) / visible_tracks as f32).min(180.0);
            let mut grid = row![].spacing(5).width(Length::Fill);
            for (column_index, track) in self
                .state
                .project_tracks
                .tracks
                .iter()
                .skip(first_track)
                .take(visible_tracks)
                .enumerate()
            {
                let track_color = th::track_color(track.color_index);
                let mut header = column![text(&track.name)
                    .font(PERFORM_LABEL)
                    .size(12)
                    .color(th::text())]
                .spacing(4);
                if !compact {
                    header = header.push(
                        text(if track.kind.is_midi() {
                            "MIDI"
                        } else {
                            "AUDIO"
                        })
                        .font(PERFORM_TECH)
                        .size(9)
                        .color(th::text_dim()),
                    );
                }
                let mut lane = column![
                    container(iced::widget::Space::new(Length::Fill, 3)).style(move |_| {
                        container::Style {
                            background: Some(track_color.into()),
                            ..Default::default()
                        }
                    }),
                    container(header).padding([if compact { 4 } else { 8 }, 10])
                ]
                .spacing(5)
                .width(Length::Fixed(lane_width));
                for offset in 0..6u32 {
                    let slot_row = first_row + offset;
                    let slot = self.state.perform.clips.at(track.id, slot_row);
                    let filled = slot.is_some();
                    let selected = slot.is_some_and(|clip| {
                        self.state.perform.clip_editor.selected == Some(clip.id)
                    });
                    let in_window = column_index < 4 && offset < 4;
                    let key = if in_window {
                        self.state
                            .perform
                            .input_mapping
                            .key_for(PadPosition {
                                row: offset as u8,
                                column: column_index as u8,
                            })
                            .label()
                    } else {
                        ""
                    };
                    let label = slot
                        .map(|clip| clip.name())
                        .unwrap_or(if track.kind.is_midi() {
                            "+ MIDI clip"
                        } else {
                            "+ Browser sample"
                        });
                    let message = if let Some(clip) = slot {
                        Message::Perform(PerformMsg::Clips(ClipMsg::Select(clip.id)))
                    } else if track.kind.is_midi() {
                        Message::Perform(PerformMsg::Clips(ClipMsg::CreateMidi {
                            track_id: track.id,
                            row: slot_row,
                        }))
                    } else {
                        Message::ImportLauncherClip {
                            track_id: track.id,
                            row: slot_row,
                        }
                    };
                    let slot_content: Element<'_, Message> = if compact {
                        row![
                            text(format!("{:02}", slot_row + 1))
                                .font(PERFORM_TECH)
                                .size(9)
                                .color(th::text_dim()),
                            text(super::keyboard::truncate_end(label, 18))
                                .size(10)
                                .color(if filled { th::text() } else { th::text_dim() })
                                .width(Length::Fill),
                            text(key).font(PERFORM_TECH).size(10).color(th::accent()),
                        ]
                        .spacing(6)
                        .align_y(iced::Alignment::Center)
                        .into()
                    } else {
                        column![
                            row![
                                text(if filled { "●" } else { "" })
                                    .size(9)
                                    .color(track_color),
                                text(format!("{:02}", slot_row + 1))
                                    .font(PERFORM_TECH)
                                    .size(9)
                                    .color(th::text_dim()),
                                horizontal_space(),
                                text(key).font(PERFORM_TECH).size(10).color(th::accent())
                            ],
                            text(super::keyboard::truncate_end(label, 22))
                                .size(11)
                                .color(if slot.is_some() {
                                    th::text()
                                } else {
                                    th::text_dim()
                                }),
                        ]
                        .spacing(if compact { 2 } else { 7 })
                        .into()
                    };
                    let slot_cell = button(slot_content)
                        .on_press(message)
                        .padding([if compact { 3 } else { 8 }, 10])
                        .width(Length::Fill)
                        .height(slot_height)
                        .style(move |_theme: &Theme, status| {
                            let mut style = choice_style(false, status);
                            if filled {
                                style.background = Some(
                                    th::blend(
                                        th::bg_elevated(),
                                        track_color,
                                        if selected { 0.26 } else { 0.12 },
                                    )
                                    .into(),
                                );
                            }
                            if selected {
                                style.border.color = track_color;
                                style.border.width = 2.0;
                            } else if in_window {
                                style.border.color = th::accent_dim();
                            }
                            style
                        });
                    let cell: Element<'_, Message> = if !track.kind.is_midi()
                        && slot.is_none()
                        && self.state.browser.drag_source.is_some()
                    {
                        mouse_area(slot_cell)
                            .on_release(Message::ImportLauncherClip {
                                track_id: track.id,
                                row: slot_row,
                            })
                            .into()
                    } else {
                        slot_cell.into()
                    };
                    lane = lane.push(cell);
                }
                grid = grid.push(lane);
            }
            workspace = workspace.push(
                scrollable(container(grid).padding([0, 12]).width(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        let mut footer = row![
            text("Arrow keys move the keyboard window · Shift moves four slots")
                .font(PERFORM_TECH)
                .size(10)
                .color(th::text_dim()),
            horizontal_space()
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if let Some(id) = self.state.perform.clip_editor.selected {
            if let Some(clip) = self.state.perform.clips.by_id(id) {
                footer = footer.push(
                    button(text("Rename").size(10))
                        .padding([5, 9])
                        .style(|_theme, status| choice_style(false, status))
                        .on_press(Message::View(
                            crate::domains::view::ViewMsg::StartEditingClipName(clip.track_id, id),
                        )),
                );
            }
            footer = footer.push(
                button(text("Delete clip").size(10))
                    .padding([5, 9])
                    .style(|_theme, status| choice_style(false, status))
                    .on_press(Message::Perform(PerformMsg::Clips(ClipMsg::Delete(id)))),
            );
        }
        workspace = workspace.push(container(footer).padding([if compact { 4 } else { 8 }, 12]));
        container(workspace)
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(surface)
            .into()
    }
}
