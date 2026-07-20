//! Pad Surface rendering for the Perform workspace.

use iced::widget::{
    button, center, column, container, horizontal_space, mouse_area, pick_list, row, slider, stack,
    text, tooltip,
};
use iced::{Element, Length, Shadow, Theme, Vector};

use crate::domains::perform::{
    PadPosition, PerformEditorFocus, PerformMode, PerformMsg, SixteenLevelsParameter,
};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};
use vibez_core::id::TrackId;

use super::views_perform::{perform_pad_grid_height, perform_tool_button};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstrumentTargetOption {
    track_id: TrackId,
    label: String,
}

impl std::fmt::Display for InstrumentTargetOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn perform_bank_button(
    label: &'static str,
    tooltip_label: &'static str,
    message: PerformMsg,
) -> Element<'static, Message> {
    let control = button(
        center(
            text(label)
                .font(PERFORM_TECH_STRONG)
                .size(13)
                .color(th::text_dim()),
        )
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(Message::Perform(message))
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(20.0))
    .padding(0)
    .style(|_theme: &Theme, status| {
        let engaged = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if engaged {
                    th::bg_hover()
                } else {
                    th::bg_surface()
                }
                .into(),
            ),
            text_color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: if engaged {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        }
    });
    let hint = container(
        text(tooltip_label)
            .font(PERFORM_TECH)
            .size(9)
            .color(th::text()),
    )
    .padding([4, 6])
    .style(|_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            color: th::border_light(),
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    });
    tooltip(control, hint, tooltip::Position::Bottom)
        .gap(5)
        .padding(0)
        .into()
}

fn instrument_toggle(
    label: &'static str,
    active: bool,
    enabled: bool,
    message: PerformMsg,
) -> Element<'static, Message> {
    let content = center(text(label).font(PERFORM_LABEL).size(9).color(if enabled {
        if active {
            th::accent()
        } else {
            th::text_dim()
        }
    } else {
        th::text_muted()
    }))
    .width(Length::Fill)
    .height(Length::Fill);
    let control = button(content)
        .width(Length::FillPortion(1))
        .height(Length::Fixed(27.0))
        .padding(0)
        .style(move |_theme: &Theme, status| {
            let engaged = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if !enabled {
                        th::bg_dark()
                    } else if active {
                        th::perform_active_surface()
                    } else if engaged {
                        th::bg_hover()
                    } else {
                        th::bg_surface()
                    }
                    .into(),
                ),
                text_color: if active && enabled {
                    th::accent()
                } else {
                    th::text_dim()
                },
                border: iced::Border {
                    color: if active && enabled {
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
    if enabled {
        control.on_press(Message::Perform(message)).into()
    } else {
        control.into()
    }
}

impl App {
    fn view_instrument_controls(&self) -> Element<'_, Message> {
        let full_level_enabled = self.state.perform.full_level_enabled();
        let full_level_available = self.state.perform.full_level_available();
        let levels_enabled = self.state.perform.sixteen_levels_enabled();
        let parameter = self.state.perform.sixteen_levels_parameter();
        let range = self.state.perform.sixteen_levels_range();
        let bounds = self.state.perform.sixteen_levels_bounds();
        let choosing_source = self.state.perform.choosing_sixteen_levels_source();
        let source = self
            .state
            .perform
            .sixteen_levels_source_pitch()
            .map(crate::widgets::piano_roll::pitch_name);

        let toggles = row![
            instrument_toggle(
                "FULL LEVEL",
                full_level_enabled && full_level_available,
                full_level_available,
                PerformMsg::ToggleFullLevel,
            ),
            instrument_toggle(
                "16 LEVELS",
                levels_enabled,
                true,
                PerformMsg::ToggleSixteenLevels,
            ),
        ]
        .spacing(5);

        let assignment = pick_list(
            SixteenLevelsParameter::ALL.to_vec(),
            Some(parameter),
            |parameter| Message::Perform(PerformMsg::SelectSixteenLevelsParameter(parameter)),
        )
        .width(Length::FillPortion(1))
        .padding([4, 7])
        .text_size(9)
        .style(|_theme: &Theme, status| {
            let engaged = matches!(
                status,
                pick_list::Status::Hovered | pick_list::Status::Opened
            );
            pick_list::Style {
                text_color: th::text(),
                placeholder_color: th::text_dim(),
                handle_color: if engaged {
                    th::accent()
                } else {
                    th::text_dim()
                },
                background: th::perform_pad_lowlight().into(),
                border: iced::Border {
                    color: if engaged {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
            }
        });
        let source_label = if choosing_source {
            "CHOOSE SOURCE · PRESS PAD".to_string()
        } else if let Some(source) = source {
            format!("SOURCE · {source}")
        } else {
            "SOURCE · PLAY A PAD".to_string()
        };
        let source_button = button(
            center(text(source_label).font(PERFORM_TECH_STRONG).size(8).color(
                if choosing_source {
                    th::accent()
                } else {
                    th::text_dim()
                },
            ))
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::FillPortion(2))
        .height(Length::Fixed(27.0))
        .padding(0)
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(
                if choosing_source {
                    th::perform_active_surface()
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::bg_hover()
                } else {
                    th::bg_surface()
                }
                .into(),
            ),
            text_color: th::text_dim(),
            border: iced::Border {
                color: if choosing_source {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
        let source_button: Element<'_, Message> = if levels_enabled {
            source_button
                .on_press(Message::Perform(PerformMsg::ChooseSixteenLevelsSource))
                .into()
        } else {
            source_button.into()
        };

        let range_units = match parameter {
            SixteenLevelsParameter::Pitch => "ST",
            SixteenLevelsParameter::Velocity => "VEL",
        };
        let minimum = row![
            text(format!("MIN {} {range_units}", range.minimum))
                .font(PERFORM_TECH)
                .size(8)
                .color(th::text_dim()),
            slider(bounds.minimum..=range.maximum, range.minimum, |value| {
                Message::Perform(PerformMsg::SetSixteenLevelsMinimum(value))
            })
            .step(1_i16),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .width(Length::FillPortion(1));
        let maximum = row![
            text(format!("MAX {} {range_units}", range.maximum))
                .font(PERFORM_TECH)
                .size(8)
                .color(th::text_dim()),
            slider(range.minimum..=bounds.maximum, range.maximum, |value| {
                Message::Perform(PerformMsg::SetSixteenLevelsMaximum(value))
            })
            .step(1_i16),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .width(Length::FillPortion(1));

        container(
            column![
                toggles,
                row![assignment, source_button].spacing(5),
                row![minimum, maximum].spacing(10)
            ]
            .spacing(5),
        )
        .padding([6, 7])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_dark().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
    }

    pub(super) fn view_pad_surface(&self, surface_width: f32) -> Element<'_, Message> {
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
        let (bank, origin) = match mode {
            PerformMode::Sections => {
                let pending = self
                    .state
                    .perform
                    .pending_section_boundary_samples
                    .map(|samples| {
                        let beat = samples as f64 * self.state.transport.bpm
                            / (self.state.transport.sample_rate as f64 * 60.0);
                        format!(" · QUEUED @ BEAT {beat:.2}")
                    })
                    .unwrap_or_default();
                (
                    self.state.perform.banks.sections + 1,
                    format!("ORDER TOP-LEFT{pending}"),
                )
            }
            PerformMode::TrackMutes => (
                self.state.perform.banks.track_mutes + 1,
                "PROJECT TRACKS".to_string(),
            ),
            PerformMode::Instrument => (
                self.state.perform.banks.instrument + 1,
                "ORDER BOTTOM-LEFT".to_string(),
            ),
        };
        let header: Element<'_, Message> = if mode == PerformMode::Instrument {
            let targets: Vec<_> = self
                .state
                .project_tracks
                .tracks
                .iter()
                .filter(|track| track.is_playable_midi_target())
                .map(|track| InstrumentTargetOption {
                    track_id: track.id,
                    label: track.name.clone(),
                })
                .collect();
            let selected = self.state.arrangement.selected_track.and_then(|selected| {
                targets
                    .iter()
                    .find(|target| target.track_id == selected)
                    .cloned()
            });
            let selector = pick_list(targets, selected, |target| {
                Message::Perform(PerformMsg::SelectInstrumentTarget(target.track_id))
            })
            .placeholder("CHOOSE PLAYABLE MIDI TARGET")
            .width(Length::Fill)
            .padding([5, 8])
            .text_size(10)
            .style(|_theme: &Theme, status| {
                let engaged = matches!(
                    status,
                    pick_list::Status::Hovered | pick_list::Status::Opened
                );
                pick_list::Style {
                    text_color: th::text(),
                    placeholder_color: th::text_dim(),
                    handle_color: if engaged {
                        th::accent()
                    } else {
                        th::text_dim()
                    },
                    background: th::perform_pad_lowlight().into(),
                    border: iced::Border {
                        color: if engaged {
                            th::accent_dim()
                        } else {
                            th::border_light()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                }
            })
            .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
                background: th::bg_elevated().into(),
                border: iced::Border {
                    color: th::border_light(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color: th::text(),
                selected_text_color: th::accent(),
                selected_background: th::bg_hover().into(),
            });
            let target_overlay = self.state.perform.instrument_target_overlay;
            let range_or_bank = if target_overlay {
                format!("TARGET BANK {bank}")
            } else if self.state.perform.choosing_sixteen_levels_source() {
                "16 LEVELS · CHOOSE SOURCE".to_string()
            } else if self.state.perform.sixteen_levels_enabled() {
                let range = self.state.perform.sixteen_levels_range();
                format!(
                    "16 LEVELS · {} {}–{}",
                    self.state.perform.sixteen_levels_parameter(),
                    range.minimum,
                    range.maximum
                )
            } else {
                let low = crate::widgets::piano_roll::pitch_name(
                    self.state.perform.instrument_pitch(PadPosition::ALL[12]),
                );
                let high = crate::widgets::piano_roll::pitch_name(
                    self.state.perform.instrument_pitch(PadPosition::ALL[3]),
                );
                format!(
                    "OCTAVE {:+} · {low}–{high}",
                    self.state.perform.instrument_octave()
                )
            };
            let previous_hint = if target_overlay {
                "PREVIOUS TARGET BANK"
            } else {
                "OCTAVE DOWN  ["
            };
            let next_hint = if target_overlay {
                "NEXT TARGET BANK"
            } else {
                "OCTAVE UP  ]"
            };
            let target_navigation = row![
                text("INSTRUMENT TARGET")
                    .font(PERFORM_LABEL)
                    .size(8)
                    .color(th::text_muted()),
                horizontal_space(),
                text(range_or_bank)
                    .font(PERFORM_TECH_STRONG)
                    .size(8)
                    .color(th::blend(th::text_dim(), th::text(), 0.24)),
                perform_bank_button("‹", previous_hint, PerformMsg::PreviousBank),
                perform_bank_button("›", next_hint, PerformMsg::NextBank),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center);
            let target_panel = column![target_navigation, selector]
                .spacing(4)
                .width(Length::Fill);

            row![heading, target_panel]
                .spacing(20)
                .align_y(iced::Alignment::End)
                .into()
        } else {
            let bank_navigation = row![
                text(format!("BANK {bank}"))
                    .font(PERFORM_TECH_STRONG)
                    .size(9)
                    .color(th::blend(th::text_dim(), th::text(), 0.24)),
                perform_bank_button("‹", "PREVIOUS BANK  [", PerformMsg::PreviousBank),
                perform_bank_button("›", "NEXT BANK  ]", PerformMsg::NextBank),
                text(format!("· {origin}"))
                    .font(PERFORM_TECH)
                    .size(9)
                    .color(th::blend(th::text_dim(), th::text(), 0.24)),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center);
            row![heading, horizontal_space(), bank_navigation]
                .align_y(iced::Alignment::End)
                .into()
        };

        let pad_grid_height = (perform_pad_grid_height(self.state.view.window_height)
            - if mode == PerformMode::Instrument {
                82.0
            } else {
                0.0
            })
        .max(200.0);
        let mut grid = column![]
            .width(Length::Fill)
            .height(Length::Fixed(pad_grid_height))
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
        let grid = center(grid)
            .width(Length::Fill)
            .height(Length::Fixed(pad_grid_height));

        let content = if mode == PerformMode::Instrument {
            column![header, self.view_instrument_controls(), grid].spacing(8)
        } else {
            column![header, grid].spacing(12)
        };
        let surface = container(content)
            .width(Length::Fixed(surface_width))
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
        let visible_bank =
            if mode == PerformMode::Instrument && !self.state.perform.instrument_target_overlay {
                0
            } else {
                self.state.perform.banks.for_mode(mode)
            };
        let ordinal = u16::from(position.ordinal(mode)) + u16::from(visible_bank) * 16;
        let section = (mode == PerformMode::Sections)
            .then(|| self.state.perform.sections.at_slot(ordinal - 1))
            .flatten();
        let selected = section
            .is_some_and(|section| self.state.perform.selected_section == Some(section.id))
            || self.state.perform.selected_pad == Some(position);
        let playing =
            section.is_some_and(|section| self.state.perform.playing_section == Some(section.id));
        let queued =
            section.is_some_and(|section| self.state.perform.queued_section == Some(section.id));
        let playhead_fraction = section.filter(|_| playing).map(|section| {
            super::views_perform_playhead::section_playhead_fraction(
                self.state.perform.section_playhead_samples,
                section.length_beats,
                self.state.transport.bpm,
                self.state.transport.sample_rate,
            )
        });
        let pressed = self.state.perform.is_pad_pressed(position);
        let mute_track = (mode == PerformMode::TrackMutes)
            .then(|| {
                self.state
                    .perform
                    .track_for_mute_pad(position, &self.state.project_tracks.tracks)
            })
            .flatten();
        let instrument_target = (mode == PerformMode::Instrument
            && self.state.perform.instrument_target_overlay)
            .then(|| {
                self.state
                    .perform
                    .track_for_instrument_target_pad(position, &self.state.project_tracks.tracks)
            })
            .flatten();
        let selected_instrument = self.state.arrangement.selected_track.and_then(|track_id| {
            self.state
                .project_tracks
                .tracks
                .iter()
                .find(|track| track.id == track_id && track.is_playable_midi_target())
        });
        let selected = selected
            || instrument_target
                .is_some_and(|track| self.state.arrangement.selected_track == Some(track.id));
        let (title, detail, color, muted) = match mode {
            PerformMode::Sections => match section {
                Some(section) => (
                    section.name.clone(),
                    format!(
                        "{} · {:.0} BARS",
                        if playing {
                            "PLAYING"
                        } else if queued {
                            "QUEUED"
                        } else {
                            "AVAILABLE"
                        },
                        section.length_beats / 4.0
                    ),
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
            PerformMode::Instrument if self.state.perform.instrument_target_overlay => {
                if let Some(track) = instrument_target {
                    (
                        track.name.clone(),
                        "SELECT INSTRUMENT TARGET".to_string(),
                        th::track_color(track.color_index),
                        false,
                    )
                } else {
                    (
                        "—".to_string(),
                        "NO PLAYABLE MIDI TARGET".to_string(),
                        th::text_muted(),
                        false,
                    )
                }
            }
            PerformMode::Instrument => {
                let preview = self.state.perform.instrument_pad_preview(position);
                let choosing_source = self.state.perform.choosing_sixteen_levels_source();
                let levels_enabled = self.state.perform.sixteen_levels_enabled();
                let parameter = self.state.perform.sixteen_levels_parameter();
                let target = selected_instrument
                    .map(|track| track.name.clone())
                    .unwrap_or_else(|| "NO INSTRUMENT TARGET".to_string());
                let detail = if choosing_source {
                    format!("CHOOSE SOURCE · {target}")
                } else if levels_enabled && parameter == SixteenLevelsParameter::Velocity {
                    format!("VELOCITY {} · {target}", preview.velocity)
                } else if levels_enabled {
                    format!("PITCH LEVEL · {target}")
                } else {
                    target
                };
                (
                    crate::widgets::piano_roll::pitch_name(preview.pitch),
                    detail,
                    selected_instrument
                        .map(|track| th::track_color(track.color_index))
                        .unwrap_or_else(|| th::track_color((ordinal - 1) as u8)),
                    false,
                )
            }
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
                        if pressed || playing {
                            th::blend(th::accent_dim(), color, 0.35)
                        } else if queued {
                            th::blend(th::accent_dim(), th::bg_hover(), 0.42)
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
                color: if pressed || playing {
                    th::accent()
                } else if queued || selected {
                    th::accent_dim()
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
            (PerformMode::Sections, Some(section)) => {
                let select = mouse_area(pad)
                    .on_press(Message::Perform(PerformMsg::SelectSection(section.id)));
                let launch = container(perform_tool_button(
                    icons::PLAY,
                    "Launch Section",
                    Message::Perform(PerformMsg::LaunchSection(section.id)),
                    playing,
                    false,
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(8);
                if let Some(fraction) = playhead_fraction {
                    stack![
                        select,
                        super::views_perform_playhead::pad_playhead(fraction),
                        launch
                    ]
                    .into()
                } else {
                    stack![select, launch].into()
                }
            }
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
            (PerformMode::Instrument, _) if instrument_target.is_some() => {
                let track_id = instrument_target.expect("checked target").id;
                mouse_area(pad)
                    .on_press(Message::Perform(PerformMsg::SelectInstrumentTarget(
                        track_id,
                    )))
                    .into()
            }
            _ => pad,
        }
    }
}
