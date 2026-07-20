//! Instrument-mode controls for the Perform pad surface.

use std::borrow::Borrow;

use iced::widget::{
    button, column, container, horizontal_space, pick_list, row, slider, text, vertical_space,
};
use iced::{Element, Length, Theme};

use vibez_core::id::TrackId;
use vibez_core::perform::NoteRepeatRate;

use crate::domains::perform::{PerformMsg, SixteenLevelsParameter};
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

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

fn rail_button(
    label: &'static str,
    state: String,
    active: bool,
    enabled: bool,
    message: PerformMsg,
) -> Element<'static, Message> {
    let foreground = if !enabled {
        th::text_muted()
    } else if active {
        th::accent()
    } else {
        th::text_dim()
    };
    let content = container(
        column![
            text(label).font(PERFORM_LABEL).size(8).color(foreground),
            text(state)
                .font(PERFORM_TECH_STRONG)
                .size(8)
                .color(foreground),
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Left)
    .align_y(iced::alignment::Vertical::Center);
    let control = button(content)
        .width(Length::FillPortion(1))
        .height(Length::Fixed(50.0))
        .padding([0, 8])
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
                text_color: foreground,
                border: iced::Border {
                    color: if active && enabled {
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
    if enabled {
        control.on_press(Message::Perform(message)).into()
    } else {
        control.into()
    }
}

fn range_control<'a>(
    label: String,
    range: std::ops::RangeInclusive<i16>,
    value: i16,
    on_change: impl Fn(i16) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).font(PERFORM_TECH).size(8).color(th::text_dim()),
        slider(range, value, on_change).step(1_i16),
    ]
    .spacing(3)
    .into()
}

fn navigation_button(label: &'static str, message: PerformMsg) -> Element<'static, Message> {
    button(
        container(
            text(label)
                .font(PERFORM_TECH_STRONG)
                .size(12)
                .color(th::text_dim()),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center),
    )
    .on_press(Message::Perform(message))
    .width(Length::Fixed(25.0))
    .height(Length::Fixed(22.0))
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
            text_color: if engaged {
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
    })
    .into()
}

fn compact_pick_list<'a, T>(
    options: impl Borrow<[T]> + 'a,
    selected: Option<T>,
    on_selected: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_selected)
        .width(Length::Fill)
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
                    radius: 2.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

fn rail_nudge_button(label: &'static str, message: PerformMsg) -> Element<'static, Message> {
    button(
        text(label)
            .font(PERFORM_TECH_STRONG)
            .size(10)
            .color(th::text_dim()),
    )
    .on_press(Message::Perform(message))
    .width(Length::Fixed(24.0))
    .height(Length::Fixed(22.0))
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
            text_color: if engaged {
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
    })
    .into()
}

impl App {
    pub(super) fn view_instrument_control_rail(
        &self,
        bank: u8,
        height: f32,
    ) -> iced::widget::Container<'_, Message> {
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
        let selected = self.state.perform.instrument_target().and_then(|selected| {
            targets
                .iter()
                .find(|target| target.track_id == selected)
                .cloned()
        });
        let selector = pick_list(targets, selected, |target| {
            Message::Perform(PerformMsg::SelectInstrumentTarget(target.track_id))
        })
        .placeholder("CHOOSE MIDI TARGET")
        .width(Length::Fill)
        .padding([5, 7])
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
                        th::border_light()
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        });

        let target_overlay = self.state.perform.instrument_target_overlay;
        let range_or_bank = if target_overlay {
            format!("TARGET BANK {bank}")
        } else {
            format!("OCTAVE {:+}", self.state.perform.instrument_octave())
        };
        let navigation = row![
            text(range_or_bank)
                .font(PERFORM_TECH_STRONG)
                .size(8)
                .color(th::text_dim()),
            horizontal_space(),
            navigation_button("‹", PerformMsg::PreviousBank),
            navigation_button("›", PerformMsg::NextBank),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center);

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
            rail_button(
                "FULL LEVEL",
                if !full_level_available {
                    "UNAVAILABLE"
                } else if full_level_enabled {
                    "VELOCITY 127"
                } else {
                    "OFF"
                }
                .into(),
                full_level_enabled && full_level_available,
                full_level_available,
                PerformMsg::ToggleFullLevel,
            ),
            rail_button(
                "16 LEVELS",
                if choosing_source {
                    "CHOOSE SOURCE".into()
                } else if levels_enabled {
                    parameter.to_string().to_uppercase()
                } else {
                    "OFF".into()
                },
                levels_enabled,
                true,
                PerformMsg::ToggleSixteenLevels,
            ),
        ]
        .spacing(5);

        let repeat_active = self.state.perform.note_repeat_active();
        let repeat_latched = self.state.perform.note_repeat_latched();
        let repeat_rate = self.state.perform.note_repeat_rate();
        let repeat_controls = row![
            rail_button(
                "NOTE REPEAT",
                if repeat_latched {
                    "LATCHED"
                } else if repeat_active {
                    "HELD"
                } else {
                    "OFF · N"
                }
                .into(),
                repeat_active,
                true,
                PerformMsg::ToggleNoteRepeatLatch,
            ),
            container(
                column![
                    text("REPEAT RATE")
                        .font(PERFORM_LABEL)
                        .size(8)
                        .color(th::text_dim()),
                    compact_pick_list(NoteRepeatRate::ALL, Some(repeat_rate), |rate| {
                        Message::Perform(PerformMsg::SetNoteRepeatRate(rate))
                    }),
                ]
                .spacing(4),
            )
            .width(Length::FillPortion(1))
            .height(Length::Fixed(50.0))
            .padding([5, 7])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }),
        ]
        .spacing(5);

        let assignment = compact_pick_list(
            SixteenLevelsParameter::ALL.to_vec(),
            Some(parameter),
            |parameter| Message::Perform(PerformMsg::SelectSixteenLevelsParameter(parameter)),
        );
        let assignment_control = container(
            column![
                text("ASSIGNMENT")
                    .font(PERFORM_LABEL)
                    .size(8)
                    .color(th::text_dim()),
                assignment,
            ]
            .spacing(4),
        )
        .width(Length::FillPortion(1))
        .height(Length::Fixed(50.0))
        .padding([5, 7])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });
        let source_state = if choosing_source {
            "PRESS A PAD".to_string()
        } else if let Some(source) = source {
            source
        } else {
            "PLAY A PAD".to_string()
        };
        let source_control = rail_button(
            "SOURCE",
            source_state,
            choosing_source,
            levels_enabled,
            PerformMsg::ChooseSixteenLevelsSource,
        );
        let range_units = match parameter {
            SixteenLevelsParameter::Pitch => "ST",
            SixteenLevelsParameter::Velocity => "VEL",
        };

        let levels_detail: Element<'_, Message> = if levels_enabled {
            column![
                row![assignment_control, source_control].spacing(5),
                row![
                    range_control(
                        format!("MIN {} {range_units}", range.minimum),
                        bounds.minimum..=range.maximum,
                        range.minimum,
                        |value| Message::Perform(PerformMsg::SetSixteenLevelsMinimum(value)),
                    ),
                    range_control(
                        format!("MAX {} {range_units}", range.maximum),
                        range.minimum..=bounds.maximum,
                        range.maximum,
                        |value| Message::Perform(PerformMsg::SetSixteenLevelsMaximum(value)),
                    ),
                ]
                .spacing(7),
            ]
            .spacing(5)
            .into()
        } else {
            container(
                text("16 LEVEL DETAILS APPEAR WHEN ENGAGED")
                    .font(PERFORM_TECH)
                    .size(7)
                    .color(th::text_muted()),
            )
            .padding([6, 0])
            .into()
        };

        let selected_track = self.state.perform.instrument_target().and_then(|track_id| {
            self.state
                .project_tracks
                .tracks
                .iter()
                .find(|track| track.id == track_id)
        });
        let project_swing = self.state.perform.project_swing();
        let track_offset = selected_track.and_then(|track| track.swing_offset);
        let effective_swing = project_swing.effective(track_offset);
        let offset = track_offset.unwrap_or_default().get();
        let swing_state = match track_offset {
            Some(_) => format!(
                "{:+.0}% · {:.0}% OUT",
                offset * 100.0,
                effective_swing.get() * 100.0
            ),
            None => format!("PROJECT · {:.0}%", effective_swing.get() * 100.0),
        };
        let track_swing = container(
            column![
                row![
                    text("TRACK SWING")
                        .font(PERFORM_LABEL)
                        .size(8)
                        .color(th::text_muted()),
                    horizontal_space(),
                    text(swing_state)
                        .font(PERFORM_TECH_STRONG)
                        .size(7)
                        .color(th::text_dim()),
                ]
                .align_y(iced::Alignment::Center),
                row![
                    rail_nudge_button("−", PerformMsg::SetTrackSwingOffset(Some(offset - 0.01)),),
                    rail_nudge_button("+", PerformMsg::SetTrackSwingOffset(Some(offset + 0.01)),),
                    horizontal_space(),
                    button(text("USE PROJECT").font(PERFORM_TECH).size(7).color(
                        if track_offset.is_some() {
                            th::text_dim()
                        } else {
                            th::text_muted()
                        }
                    ),)
                    .on_press_maybe(
                        track_offset
                            .map(|_| { Message::Perform(PerformMsg::SetTrackSwingOffset(None)) })
                    )
                    .padding([4, 6])
                    .style(move |_theme: &Theme, status| {
                        let enabled = track_offset.is_some();
                        let engaged = enabled
                            && matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: Some(
                                if engaged {
                                    th::bg_hover()
                                } else {
                                    th::bg_surface()
                                }
                                .into(),
                            ),
                            text_color: if engaged {
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
                    }),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(5),
        )
        .padding(7)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

        container(
            column![
                text("INSTRUMENT TARGET")
                    .font(PERFORM_LABEL)
                    .size(8)
                    .color(th::text_muted()),
                selector,
                navigation,
                toggles,
                repeat_controls,
                levels_detail,
                vertical_space().height(Length::Fill),
                track_swing,
            ]
            .spacing(5),
        )
        .height(Length::Fixed(height))
        .padding(7)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_dark().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
    }
}
