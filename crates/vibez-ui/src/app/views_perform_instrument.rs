//! Instrument-mode controls for the Perform pad surface.

use iced::widget::{
    button, column, container, horizontal_space, pick_list, row, slider, text, vertical_space,
};
use iced::{Element, Length, Theme};

use vibez_core::id::TrackId;

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
        let previous_hint = if target_overlay {
            "TARGET ‹"
        } else {
            "OCTAVE ‹"
        };
        let next_hint = if target_overlay {
            "TARGET ›"
        } else {
            "OCTAVE ›"
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

        let assignment = pick_list(
            SixteenLevelsParameter::ALL.to_vec(),
            Some(parameter),
            |parameter| Message::Perform(PerformMsg::SelectSixteenLevelsParameter(parameter)),
        )
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

        container(
            column![
                text("INSTRUMENT TARGET")
                    .font(PERFORM_LABEL)
                    .size(8)
                    .color(th::text_muted()),
                selector,
                navigation,
                toggles,
                column![
                    text("16 LEVEL ASSIGNMENT")
                        .font(PERFORM_LABEL)
                        .size(8)
                        .color(th::text_muted()),
                    assignment,
                ]
                .spacing(3),
                source_control,
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
                vertical_space().height(Length::Fill),
                text(format!("{previous_hint}  ·  {next_hint}"))
                    .font(PERFORM_TECH)
                    .size(7)
                    .color(th::text_muted()),
            ]
            .spacing(7),
        )
        .height(Length::Fixed(height))
        .padding(8)
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
