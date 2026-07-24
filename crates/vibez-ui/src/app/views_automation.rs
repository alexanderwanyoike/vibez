//! Shared automation-lane presentation for Arrange and Section timelines.

use iced::widget::{button, canvas, column, container, horizontal_space, row, text};
use iced::{Element, Length, Theme};

use crate::domains::automation::{target_label_with_buses, AutomationMsg};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::automation_lane::{AutomationLaneWidget, LANE_HEIGHT};

use super::*;

const MAX_PICKER_RESULTS: usize = 40;
const PICKER_CLOSED_HEIGHT: f32 = 32.0;
const PICKER_OPEN_HEIGHT: f32 = 202.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct AutomationLaneLayout {
    pub header_width: f32,
    pub body_width: Option<f32>,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
}

pub(super) struct AutomationLanePart<'a> {
    pub header: Element<'a, Message>,
    pub body: Element<'a, Message>,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct LaneChoice {
    label: String,
    target: vibez_core::automation::AutomationTarget,
}

impl App {
    pub(super) fn automation_lane_parts<'a>(
        &'a self,
        track: &'a crate::state::ProjectTrack,
        automation: &[vibez_core::automation::AutomationLane],
        track_color: iced::Color,
        layout: AutomationLaneLayout,
    ) -> Vec<AutomationLanePart<'a>> {
        let body_width = || layout.body_width.map_or(Length::Fill, Length::Fixed);
        let mut parts = Vec::with_capacity(automation.len() + 1);

        for lane in automation {
            let label =
                target_label_with_buses(&lane.target, track, &self.state.project_tracks.buses);
            let remove = button(icons::icon(icons::TRASH_2).size(9).color(th::text_dim()))
                .on_press(Message::Automation(AutomationMsg::RemoveLane {
                    track_id: track.id,
                    lane_id: lane.id,
                }))
                .padding([1, 4])
                .style(|_theme: &Theme, _status| button::Style {
                    background: None,
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                });
            let header = container(
                row![
                    text(label).size(11).color(th::text_dim()),
                    horizontal_space(),
                    remove
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding([0, 10])
            .width(Length::Fixed(layout.header_width))
            .height(Length::Fixed(LANE_HEIGHT))
            .align_y(iced::alignment::Vertical::Center)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::divider(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

            let selected = match self.state.automation_ui.selected {
                Some((t, l, i)) if t == track.id && l == lane.id => Some(i),
                _ => None,
            };
            let (reference, min_label, max_label, ref_label) = lane_scale(track, &lane.target);
            let body = canvas(AutomationLaneWidget {
                track_id: track.id,
                lane_id: lane.id,
                points: lane.points.clone(),
                color: track_color,
                zoom_level: layout.zoom_level,
                scroll_offset_beats: layout.scroll_offset_beats,
                grid: self.state.view.grid_config(),
                selected,
                reference,
                min_label,
                max_label,
                ref_label,
            })
            .width(body_width())
            .height(Length::Fixed(LANE_HEIGHT));

            parts.push(AutomationLanePart {
                header: header.into(),
                body: body.into(),
                height: LANE_HEIGHT,
            });
        }

        parts.push(self.automation_picker_part(track, automation, layout));
        parts
    }

    fn automation_picker_part<'a>(
        &'a self,
        track: &'a crate::state::ProjectTrack,
        automation: &[vibez_core::automation::AutomationLane],
        layout: AutomationLaneLayout,
    ) -> AutomationLanePart<'a> {
        let picker_query = match &self.state.automation_ui.picker {
            Some((track_id, query)) if *track_id == track.id => Some(query.clone()),
            _ => None,
        };
        let picker_open = picker_query.is_some();
        let height = if picker_open {
            PICKER_OPEN_HEIGHT
        } else {
            PICKER_CLOSED_HEIGHT
        };
        let header = container(
            text(if picker_open {
                "ADD AUTOMATION"
            } else {
                "AUTOMATION"
            })
            .size(9)
            .color(th::text_muted()),
        )
        .padding([0, 10])
        .width(Length::Fixed(layout.header_width))
        .height(Length::Fixed(height))
        .align_y(iced::alignment::Vertical::Center)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::divider(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

        let body: Element<'a, Message> = if let Some(query) = picker_query {
            let mut choices = self.automation_lane_choices(track, automation);
            let needle = query.to_lowercase();
            let total_before = choices.len();
            if !needle.is_empty() {
                choices.retain(|choice| choice.label.to_lowercase().contains(&needle));
            }
            let shown = choices.len().min(MAX_PICKER_RESULTS);
            let hidden = choices.len() - shown;

            let search = iced::widget::text_input("Search parameters…", &query)
                .on_input(|value| Message::Automation(AutomationMsg::LanePickerQuery(value)))
                .size(11)
                .padding([4, 8])
                .style(|_theme: &Theme, _status| iced::widget::text_input::Style {
                    background: th::bg_dark().into(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    icon: th::text_dim(),
                    placeholder: th::text_dim(),
                    value: th::text(),
                    selection: th::accent(),
                });
            let close = button(icons::icon(icons::X).size(10).color(th::text_dim()))
                .on_press(Message::Automation(AutomationMsg::CloseLanePicker))
                .padding([3, 6])
                .style(|_theme: &Theme, _status| button::Style {
                    background: None,
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                });

            let mut list = column![].spacing(1);
            for choice in choices.into_iter().take(MAX_PICKER_RESULTS) {
                list = list.push(
                    button(text(choice.label).size(11).color(th::text()))
                        .on_press(Message::Automation(AutomationMsg::AddLane {
                            track_id: track.id,
                            target: choice.target,
                        }))
                        .width(Length::Fill)
                        .padding([3, 10])
                        .style(|_theme: &Theme, status| button::Style {
                            background: matches!(
                                status,
                                button::Status::Hovered | button::Status::Pressed
                            )
                            .then(|| th::bg_hover().into()),
                            text_color: th::text(),
                            border: iced::Border::default(),
                            ..Default::default()
                        }),
                );
            }
            if hidden > 0 {
                list = list.push(
                    container(
                        text(format!("{hidden} more — refine the search"))
                            .size(10)
                            .color(th::text_dim()),
                    )
                    .padding([3, 10]),
                );
            }
            if total_before == 0 {
                list = list.push(
                    container(
                        text("Everything is already automated")
                            .size(10)
                            .color(th::text_dim()),
                    )
                    .padding([3, 10]),
                );
            }

            container(
                column![
                    row![search, close]
                        .spacing(6)
                        .align_y(iced::Alignment::Center),
                    iced::widget::scrollable(list).height(Length::Fixed(150.0)),
                ]
                .spacing(6),
            )
            .padding(8)
            .width(layout.body_width.map_or(Length::Fill, Length::Fixed))
            .height(Length::Fixed(height))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
        } else {
            container(
                button(text("+ Add automation").size(11).color(th::text_dim()))
                    .on_press(Message::Automation(AutomationMsg::OpenLanePicker(track.id)))
                    .width(Length::Fill)
                    .padding([3, 10])
                    .style(|_theme: &Theme, status| button::Style {
                        background:
                            Some(
                                if matches!(
                                    status,
                                    button::Status::Hovered | button::Status::Pressed
                                ) {
                                    th::bg_hover()
                                } else {
                                    th::bg_elevated()
                                }
                                .into(),
                            ),
                        text_color: th::text_dim(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .padding([2, 10])
            .width(layout.body_width.map_or(Length::Fill, Length::Fixed))
            .height(Length::Fixed(height))
            .into()
        };

        AutomationLanePart {
            header: header.into(),
            body,
            height,
        }
    }

    fn automation_lane_choices(
        &self,
        track: &crate::state::ProjectTrack,
        automation: &[vibez_core::automation::AutomationLane],
    ) -> Vec<LaneChoice> {
        let mut choices = vec![
            LaneChoice {
                label: "Volume".into(),
                target: vibez_core::automation::AutomationTarget::TrackGain,
            },
            LaneChoice {
                label: "Pan".into(),
                target: vibez_core::automation::AutomationTarget::TrackPan,
            },
        ];
        if track.kind.is_midi() {
            choices.push(LaneChoice {
                label: "Track Swing".into(),
                target: vibez_core::automation::AutomationTarget::TrackSwingOffset,
            });
        }
        if !track.plugin_instrument_descriptors.is_empty() {
            let name = track
                .plugin_instrument_name
                .clone()
                .unwrap_or_else(|| "Plugin".into());
            for (param_index, descriptor) in track.plugin_instrument_descriptors.iter().enumerate()
            {
                choices.push(LaneChoice {
                    label: format!("{name}: {}", descriptor.name),
                    target: vibez_core::automation::AutomationTarget::InstrumentParam {
                        param_index,
                    },
                });
            }
        }
        if let Some(kind) = track.instrument_kind {
            let instrument_name = match kind {
                vibez_core::midi::InstrumentKind::SubtractiveSynth => "Synth",
                vibez_core::midi::InstrumentKind::Sampler => "Sampler",
                vibez_core::midi::InstrumentKind::DrumRack => "Drum Rack",
            };
            for (param_index, descriptor) in
                vibez_instruments::descriptors_for(kind).iter().enumerate()
            {
                choices.push(LaneChoice {
                    label: format!("{instrument_name}: {}", descriptor.name),
                    target: vibez_core::automation::AutomationTarget::InstrumentParam {
                        param_index,
                    },
                });
            }
        }
        for effect in &track.effects {
            for (param_index, descriptor) in effect.descriptors.iter().enumerate() {
                let effect_name = effect
                    .plugin_name
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", effect.effect_type));
                choices.push(LaneChoice {
                    label: format!("{effect_name}: {}", descriptor.name),
                    target: vibez_core::automation::AutomationTarget::EffectParam {
                        effect_id: effect.id,
                        param_index,
                    },
                });
            }
        }
        let is_channel = track.id.is_master()
            || self
                .state
                .project_tracks
                .buses
                .iter()
                .any(|bus| bus.id == track.id);
        if !is_channel {
            for bus in &self.state.project_tracks.buses {
                choices.push(LaneChoice {
                    label: format!("Send: {}", bus.name),
                    target: vibez_core::automation::AutomationTarget::Send { bus_id: bus.id },
                });
            }
        }
        choices.retain(|choice| !automation.iter().any(|lane| lane.target == choice.target));
        choices
    }

    pub(super) fn push_automation_lanes<'a>(
        &'a self,
        mut rows: iced::widget::Column<'a, Message>,
        timeline: &'a crate::state::TimelineEditorState,
        track: &'a crate::state::ProjectTrack,
        track_color: iced::Color,
    ) -> iced::widget::Column<'a, Message> {
        let automation = timeline
            .timeline
            .get(track.id)
            .map(|content| content.automation.as_slice())
            .unwrap_or(&[]);
        let layout = AutomationLaneLayout {
            header_width: crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            body_width: None,
            zoom_level: self.state.view.zoom_level,
            scroll_offset_beats: self.state.view.scroll_offset_beats,
        };
        for part in self.automation_lane_parts(track, automation, track_color, layout) {
            rows = rows.push(row![part.header, part.body].height(Length::Fixed(part.height)));
        }
        rows
    }
}

/// Reference value plus scale labels for a lane's target.
fn lane_scale(
    track: &crate::state::ProjectTrack,
    target: &vibez_core::automation::AutomationTarget,
) -> (Option<f32>, String, String, String) {
    use crate::domains::automation::{normalized_target_value, target_descriptor};
    use vibez_core::automation::AutomationTarget;

    let reference = normalized_target_value(target, track);
    match target {
        AutomationTarget::TrackGain => {
            let label = reference
                .map(|value| fmt_value(value * 2.0, ""))
                .unwrap_or_default();
            (reference, "0".into(), "2.0".into(), label)
        }
        AutomationTarget::TrackPan => {
            let label = match reference {
                Some(value) if (value - 0.5).abs() < 0.01 => "C".into(),
                Some(value) => fmt_value(value * 2.0 - 1.0, ""),
                None => String::new(),
            };
            (reference, "L".into(), "R".into(), label)
        }
        _ => match target_descriptor(target, track) {
            Some(descriptor) => {
                let label = reference
                    .map(|value| {
                        fmt_value(
                            descriptor.min + value * (descriptor.max - descriptor.min),
                            descriptor.unit,
                        )
                    })
                    .unwrap_or_default();
                (
                    reference,
                    fmt_value(descriptor.min, descriptor.unit),
                    fmt_value(descriptor.max, descriptor.unit),
                    label,
                )
            }
            None => (reference, String::new(), String::new(), String::new()),
        },
    }
}

fn fmt_value(value: f32, unit: &str) -> String {
    let number = if value.abs() >= 1000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else {
        let formatted = format!("{value:.2}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}
