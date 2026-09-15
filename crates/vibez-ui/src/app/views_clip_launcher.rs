//! Clip Project creation and grid authoring use the shared Vibez shell.

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable,
    stack, text, text_input,
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
        let content = if self.state.perform.mode == PerformMode::Sections {
            self.view_clip_grid(width)
        } else {
            let dock_width = width.max(900.0);
            let pad_width = super::views_perform::effective_perform_surface_width(
                self.state.view.perform_surface_width,
                dock_width,
            );
            let pad_width = self.clip_pad_surface_width(pad_width);
            let grid_width =
                dock_width - pad_width - super::views_shell::HORIZONTAL_PANE_SPLITTER_WIDTH;
            scrollable::Scrollable::with_direction(
                row![
                    self.view_pad_surface(pad_width),
                    self.view_perform_surface_splitter(),
                    container(self.view_clip_grid(grid_width)).width(grid_width),
                ]
                .width(dock_width)
                .height(Length::Fill),
                scrollable::Direction::Horizontal(scrollable::Scrollbar::default()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };
        container(column![self.view_perform_mode_selector(width), content])
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(surface)
            .into()
    }

    fn view_clip_grid(&self, width: f32) -> Element<'_, Message> {
        let keyboard_active = self.state.perform.mode == PerformMode::Sections;
        let compact = self.state.view.window_height < 800.0;
        let slot_height = if compact {
            ((self.state.view.window_height - 480.0) / 5.0).clamp(26.0, 72.0)
        } else {
            (64.0 + (self.state.view.window_height - 800.0) * 0.08).min(80.0)
        };
        let mut workspace = column![];
        let move_window = |label: &'static str, tracks, rows| {
            button(text(label).font(PERFORM_TECH).size(12))
                .on_press(Message::Perform(PerformMsg::Clips(ClipMsg::MoveWindow {
                    tracks,
                    rows,
                })))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
        };
        let heading = row![
            text(if width < 780.0 { "" } else { "CLIPS" })
                .font(PERFORM_LABEL)
                .size(11)
                .color(th::text()),
            self.view_clip_capture_button(),
            text(if width < 780.0 {
                String::new()
            } else {
                format!("{} TRACKS", self.state.project_tracks.tracks.len())
            })
            .font(PERFORM_TECH)
            .size(9)
            .color(th::text_dim()),
            horizontal_space(),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let actions = row![
            button(text("+ Audio").size(11))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
                .on_press(Message::Arrangement(ArrangementMsg::AddTrack)),
            button(text("+ MIDI").size(11))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
                .on_press(Message::Arrangement(ArrangementMsg::AddMidiTrack)),
            button(text(if width < 780.0 { "■" } else { "Stop clips" }).size(11))
                .padding([6, 10])
                .style(|_theme, status| choice_style(false, status))
                .on_press(Message::Perform(PerformMsg::Clips(ClipMsg::StopAll))),
            move_window("←", -4, 0),
            move_window("→", 4, 0),
            move_window("↑", 0, -4),
            move_window("↓", 0, 4),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let toolbar: Element<'_, Message> = if width < 600.0 {
            column![heading, actions].spacing(6).into()
        } else {
            row![heading, actions]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into()
        };
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
            let shown_tracks = self
                .state
                .project_tracks
                .tracks
                .len()
                .saturating_sub(first_track)
                .min(visible_tracks)
                .max(4);
            let lane_width = ((width - 58.0) / shown_tracks as f32 - 8.0).min(210.0);
            let header_height = if compact { 26.0 } else { 42.0 };
            let mut numbers = column![iced::widget::Space::new(1, header_height)].spacing(6);
            for offset in 0..6u32 {
                numbers = numbers.push(
                    container(
                        text(format!("{:02}", first_row + offset + 1))
                            .font(PERFORM_TECH)
                            .size(10)
                            .color(th::text_dim()),
                    )
                    .padding([if compact { 3 } else { 8 }, 0])
                    .height(slot_height),
                );
            }
            let mut grid = row![numbers.width(22)].spacing(8).width(Length::Fill);
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
                let track_name: Element<'_, Message> =
                    if self.state.view.editing_track_name == Some(track.id) {
                        self.view_launcher_name_input()
                    } else {
                        canvas(crate::widgets::clip_slot::LauncherTrackName {
                            track_id: track.id,
                            name: &track.name,
                            color: track_color,
                        })
                        .width(Length::Fill)
                        .height(20)
                        .into()
                    };
                let mut header = column![track_name].spacing(3);
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
                let mut lane = column![container(header).padding([3, 8]).height(header_height)]
                    .spacing(6)
                    .width(Length::Fixed(lane_width));
                for offset in 0..6u32 {
                    let slot_row = first_row + offset;
                    let slot = self.state.perform.clips.at(track.id, slot_row);
                    let selected = slot.is_some_and(|clip| {
                        self.state.perform.clip_editor.selected == Some(clip.id)
                    });
                    let in_window = column_index < 4 && offset < 4;
                    let key = if keyboard_active && in_window {
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
                    let slot_cell = button(
                        canvas(crate::widgets::clip_slot::ClipSlot {
                            clip: slot,
                            color: track_color,
                            key,
                            selected,
                            progress: slot.and_then(|clip| {
                                let active = self
                                    .state
                                    .perform
                                    .clip_editor
                                    .playing
                                    .get(&track.id)
                                    .filter(|active| active.id == clip.id)?;
                                let start =
                                    *self.state.perform.clip_editor.started_at.get(&track.id)?;
                                let spb = self.state.transport.sample_rate as f64 * 60.0
                                    / self.state.transport.bpm;
                                let (length, looping) = active.length_and_loop(spb);
                                let elapsed = self
                                    .state
                                    .perform
                                    .performance_position_samples
                                    .saturating_sub(start);
                                Some(if looping {
                                    (elapsed % length) as f32 / length as f32
                                } else {
                                    elapsed.min(length) as f32 / length as f32
                                })
                            }),
                            playing: slot.is_some_and(|clip| {
                                self.state
                                    .perform
                                    .clip_editor
                                    .playing
                                    .get(&track.id)
                                    .is_some_and(|active| active.id == clip.id)
                            }),
                            queued: slot.is_some_and(|clip| {
                                self.state.perform.clip_editor.queued.get(&track.id)
                                    == Some(&Some(clip.id))
                            }),
                            compact,
                        })
                        .width(Length::Fill)
                        .height(Length::Fill),
                    )
                    .on_press(message)
                    .padding(0)
                    .width(Length::Fill)
                    .height(slot_height)
                    .style(|_, _| button::Style {
                        background: None,
                        border: iced::Border::default(),
                        ..Default::default()
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
                    let cell = if slot.is_some_and(|clip| {
                        self.state.view.editing_clip_name == Some((track.id, clip.id))
                    }) {
                        stack![
                            cell,
                            container(self.view_launcher_name_input())
                                .padding([3, 6])
                                .width(Length::Fill)
                                .height(26)
                        ]
                        .into()
                    } else {
                        cell
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
            text(if keyboard_active {
                "Keys launch · Shift + key stops · Alt + key launches row · F5 Capture"
            } else {
                "F1 Clips · Double-click names to edit"
            })
            .font(PERFORM_TECH)
            .size(10)
            .color(th::text_dim()),
            horizontal_space()
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if let Some(id) = self.state.perform.clip_editor.selected {
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
            .height(Length::Fill)
            .into()
    }
}

impl App {
    fn view_launcher_name_input(&self) -> Element<'_, Message> {
        text_input("Name", &self.state.view.edit_name_text)
            .id("launcher-name")
            .on_input(|name| Message::View(crate::domains::view::ViewMsg::EditNameText(name)))
            .on_submit(Message::View(crate::domains::view::ViewMsg::FinishEditing))
            .size(12)
            .padding([2, 4])
            .width(Length::Fill)
            .into()
    }

    fn view_clip_capture_button(&self) -> Element<'_, Message> {
        let active = self.state.perform.capture.is_active();
        button(
            row![
                text(if active {
                    "● CAPTURING"
                } else {
                    "○ CAPTURE"
                })
                .font(PERFORM_LABEL)
                .size(10),
                text("F5").font(PERFORM_TECH).size(9),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([6, 10])
        .style(move |_, status| choice_style(active, status))
        .on_press(Message::Perform(PerformMsg::Capture(
            crate::domains::perform::CaptureMsg::Toggle,
        )))
        .into()
    }
}
