//! Contextual Track Swing and per-clip application in the shared Clip inspector.

use iced::widget::{button, canvas, column, container, horizontal_space, row, text, text_input};
use iced::{Color, Element, Length, Theme};

use vibez_core::automation::AutomationTarget;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::{GrooveGrid, SwingOffset};

use crate::domains::perform::PerformMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::swing_knob::{offset_for_effective_percent, SwingKnobWidget};

use super::*;

impl App {
    pub(super) fn view_midi_track_clip_placeholder(
        &self,
        track_id: TrackId,
        track_color: Color,
    ) -> Element<'_, Message> {
        let content = column![
            self.view_clip_swing_relationship(track_id, track_color, None),
            text("Select a MIDI clip to choose how it follows this Track Swing")
                .size(10)
                .color(th::text_muted()),
        ]
        .spacing(7);
        container(content)
            .padding([8, 12])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_clip_swing_relationship(
        &self,
        track_id: TrackId,
        track_color: Color,
        clip: Option<(ClipId, GrooveGrid)>,
    ) -> Element<'_, Message> {
        let Some(track) = self.state.find_track(track_id) else {
            return horizontal_space().into();
        };
        let project_swing = self.state.perform.project_swing();
        let manual_offset = track.swing_offset;
        let automation_offset = self
            .state
            .active_timeline_content(track_id)
            .and_then(|content| {
                content
                    .automation
                    .iter()
                    .find(|lane| lane.target == AutomationTarget::TrackSwingOffset)
            })
            .and_then(|lane| lane.value_at(self.state.position_beats()))
            .map(SwingOffset::from_normalized);
        let automated = automation_offset.is_some();
        let controlling_offset = automation_offset.or(manual_offset);
        let effective_swing = project_swing.effective(controlling_offset);
        let composition = match controlling_offset {
            Some(offset) if automated => {
                format!("AUTO · PROJECT {:+.0}", offset.get() * 100.0)
            }
            Some(offset) => format!("PROJECT {:+.0}", offset.get() * 100.0),
            None => "FOLLOW PROJECT".to_string(),
        };

        let knob: Element<'_, Message> = canvas(SwingKnobWidget::track(
            track_id,
            project_swing,
            effective_swing,
            automated,
        ))
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .into();

        let value: Element<'_, Message> = if automated {
            text(format!("{:.0}% AUTO", effective_swing.get() * 100.0))
                .size(10)
                .color(th::success())
                .into()
        } else {
            let input = self.state.perform.target_swing_input(track_id);
            let submit = offset_for_effective_percent(input, project_swing)
                .map(|offset| {
                    Message::Perform(PerformMsg::SetTrackSwingOffset {
                        track_id,
                        value: Some(offset.get()),
                    })
                })
                .unwrap_or_else(|| {
                    Message::Perform(PerformMsg::SetTrackSwingOffset {
                        track_id,
                        value: manual_offset.map(SwingOffset::get),
                    })
                });
            text_input(&format!("{:.0}%", effective_swing.get() * 100.0), input)
                .on_input(move |value| {
                    Message::Perform(PerformMsg::TargetSwingInput { track_id, value })
                })
                .on_submit(submit)
                .width(Length::Fixed(44.0))
                .padding([2, 4])
                .size(10)
                .into()
        };

        let follows_project = manual_offset.is_none();
        let follow = {
            let control = button(text("FOLLOW").size(7))
                .width(Length::Fixed(42.0))
                .padding([3, 4])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(
                        if follows_project {
                            th::accent_dim()
                        } else {
                            th::bg_elevated()
                        }
                        .into(),
                    ),
                    text_color: if follows_project {
                        th::accent()
                    } else {
                        th::text_dim()
                    },
                    border: iced::Border {
                        color: if follows_project {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });
            if follows_project {
                control
            } else {
                control.on_press(Message::Perform(PerformMsg::SetTrackSwingOffset {
                    track_id,
                    value: None,
                }))
            }
        };

        let amount = row![
            knob,
            column![
                row![
                    text("TRACK SWING").size(7).color(th::text_muted()),
                    text(track.name.to_uppercase()).size(7).color(track_color),
                ]
                .spacing(5),
                row![value, follow]
                    .spacing(3)
                    .align_y(iced::Alignment::Center),
                text(composition).size(7).color(if automated {
                    th::success()
                } else {
                    th::text_dim()
                }),
            ]
            .spacing(2),
        ]
        .spacing(7)
        .align_y(iced::Alignment::Center);

        let body: Element<'_, Message> = if let Some((clip_id, groove_grid)) = clip {
            let choices = GrooveGrid::ALL.into_iter().fold(
                row![].spacing(2).align_y(iced::Alignment::Center),
                |control, grid| {
                    let selected = grid == groove_grid;
                    let label = if grid == GrooveGrid::Off {
                        "OFF"
                    } else {
                        grid.label()
                    };
                    control.push(
                        button(text(label).size(8))
                            .on_press(Message::PianoRoll(PianoRollMsg::SetNoteClipGrooveGrid(
                                track_id, clip_id, grid,
                            )))
                            .padding([3, 7])
                            .style(move |_theme: &Theme, _status| button::Style {
                                background: Some(
                                    if selected {
                                        th::accent_dim()
                                    } else {
                                        th::bg_elevated()
                                    }
                                    .into(),
                                ),
                                text_color: if selected {
                                    th::accent()
                                } else {
                                    th::text_dim()
                                },
                                border: iced::Border {
                                    color: if selected { th::accent() } else { th::border() },
                                    width: 1.0,
                                    radius: 2.0.into(),
                                },
                                ..Default::default()
                            }),
                    )
                },
            );
            let explanation = if groove_grid == GrooveGrid::Off {
                "PLAYBACK UNCHANGED".to_string()
            } else {
                format!(
                    "{:.0}% FROM {}",
                    effective_swing.get() * 100.0,
                    track.name.to_uppercase()
                )
            };
            row![
                amount,
                container(horizontal_space())
                    .width(Length::Fixed(1.0))
                    .height(Length::Fixed(38.0))
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::border().into()),
                        ..Default::default()
                    }),
                column![
                    text("APPLY TO THIS CLIP").size(7).color(th::text_muted()),
                    choices,
                    text(explanation)
                        .size(7)
                        .color(if groove_grid == GrooveGrid::Off {
                            th::text_muted()
                        } else {
                            th::accent()
                        }),
                ]
                .spacing(2),
            ]
            .spacing(9)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            amount.into()
        };

        container(body)
            .padding([5, 7])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                ..Default::default()
            })
            .into()
    }
}
