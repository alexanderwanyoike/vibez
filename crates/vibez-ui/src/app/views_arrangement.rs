//! Arrangement workspace view: track lanes, clip canvases, per-track
//! automation lanes, and the minimap.
//! Split from views_shell.rs; inherent methods on [`super::App`].

use std::collections::HashSet;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable, text,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, TrackId};

use crate::icons;
use crate::message::Message;
use crate::state::{ArrangementSelection, ContextMenuTarget, TrackTimelineContent};
use crate::theme as th;
use crate::timeline_geometry::TimelineGeometry;
use crate::widgets::timeline::{ArrangementMinimap, MinimapTrack, RulerWidget, TrackClipCanvas};
use crate::widgets::track_header::{view_editable_channel_name, view_track_header};

use super::*;

impl App {
    pub(super) fn view_arrangement(&self) -> Element<'_, Message> {
        let timeline = self.state.arrangement.resolve_timeline().editor;
        if self.state.project_tracks.tracks.is_empty() {
            let browser_drag_active = self.state.browser.drag_source.is_some();
            let empty_beat = match self.state.browser.drag_target {
                Some(crate::state::BrowserDropTarget::EmptyArrangement { beat }) => Some(beat),
                _ => None,
            };
            let prompt_text = if browser_drag_active {
                empty_beat
                    .map(|beat| format!("DROP → NEW AUDIO TRACK · BEAT {beat:.2}"))
                    .unwrap_or_else(|| "DROP → NEW AUDIO TRACK".into())
            } else {
                "Right-click or Ctrl+T to add a track".into()
            };
            let prompt = text(prompt_text).size(16).color(if browser_drag_active {
                th::accent()
            } else {
                th::text_dim()
            });

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            let mut area = mouse_area(
                container(centered)
                    .width(Length::Fill)
                    .height(Length::FillPortion(5))
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::bg_dark().into()),
                        ..Default::default()
                    }),
            )
            .on_right_press(Message::View(ViewMsg::ShowContextMenu {
                x: 400.0,
                y: 300.0,
                target: ContextMenuTarget::ArrangementEmpty,
            }));
            if browser_drag_active {
                let grid = self.state.view.grid_config();
                let geometry = TimelineGeometry::from_zoom(
                    self.state.view.zoom_level,
                    self.state.view.scroll_offset_beats,
                );
                area = area
                    .on_move(move |point| {
                        let beat =
                            grid.snap_beat(geometry.x_to_beat(point.x), geometry.pixels_per_beat());
                        Message::Browser(BrowserMsg::DragHoverEmptyArrangement {
                            beat: beat.max(0.0),
                        })
                    })
                    .on_release(Message::DropSampleOnEmptyArrangement);
            }
            return area.into();
        }

        let playhead_beats = self.state.position_beats();
        let sample_rate = self.state.transport.sample_rate;
        let bpm = self.state.transport.bpm;
        let zoom_level = self.state.view.zoom_level;
        let scroll_offset = self.state.view.scroll_offset_beats;
        let geometry = TimelineGeometry::from_zoom(zoom_level, scroll_offset);
        let total_beats = self.state.total_beats();

        // Beat-based ruler across the top (offset by track header width)
        let ruler = RulerWidget {
            playhead_beats,
            bpm,
            zoom_level,
            grid: self.state.view.grid_config(),
            scroll_offset_beats: scroll_offset,
            total_beats,
            loop_enabled: self.state.transport.loop_enabled,
            loop_start_beats: self.state.transport.loop_start_beats,
            loop_end_beats: self.state.transport.loop_end_beats,
            time_selection_active: timeline.time_selection_active,
            selection_start_beats: timeline.selection_start_beats,
            selection_end_beats: timeline.selection_end_beats,
        };
        let ruler_canvas: Element<'_, Message> = canvas(ruler)
            .width(Length::Fill)
            .height(Length::Fixed(28.0))
            .into();

        // Spacer matching header width (including color bar) for the ruler row
        let ruler_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(28.0))
            .style(|_theme: &Theme| iced::widget::container::Style {
                background: Some(crate::theme::bg_surface().into()),
                ..Default::default()
            });

        let ruler_row = row![ruler_spacer, ruler_canvas];

        // Arrangement overview minimap
        let spb = if bpm > 0.0 {
            60.0 * sample_rate as f64 / bpm
        } else {
            1.0
        };
        let minimap = ArrangementMinimap {
            total_beats,
            scroll_offset_beats: scroll_offset,
            zoom_level,
            playhead_beats,
            bpm,
            loop_enabled: self.state.transport.loop_enabled,
            loop_start_beats: self.state.transport.loop_start_beats,
            loop_end_beats: self.state.transport.loop_end_beats,
            tracks: self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|t| {
                    let color = th::track_color(t.color_index);
                    let content = timeline.timeline.get(t.id);
                    let mut clips: Vec<(f64, f64)> = content
                        .into_iter()
                        .flat_map(|content| {
                            content
                                .clips
                                .iter()
                                .map(|c| (c.position as f64 / spb, c.duration as f64 / spb))
                        })
                        .collect();
                    clips.extend(content.into_iter().flat_map(|content| {
                        content
                            .note_clips
                            .iter()
                            .map(|c| (c.position_beats, c.duration_beats))
                    }));
                    MinimapTrack { color, clips }
                })
                .collect(),
        };
        let minimap_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(40.0))
            .style(|_theme: &Theme| iced::widget::container::Style {
                background: Some(th::bg_surface().into()),
                ..Default::default()
            });
        let minimap_canvas: Element<'_, Message> = canvas(minimap)
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .into();
        let minimap_row = row![minimap_spacer, minimap_canvas];

        // Collect track IDs and kinds for cross-track drag
        let track_ids: Vec<TrackId> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|t| t.id)
            .collect();
        let track_kinds: Vec<bool> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|t| t.kind.is_midi())
            .collect();
        let total_track_count = self.state.project_tracks.tracks.len();
        let browser_drag_duration = self
            .state
            .browser
            .drag_preview_beats(self.state.transport.bpm);
        let browser_drag_detail = self.state.browser.drag_label.as_ref().map(|label| {
            let mode = match self.state.browser.audition_mode {
                crate::state::AuditionMode::Raw => "RAW",
                crate::state::AuditionMode::Warp => "WARP",
            };
            match browser_drag_duration {
                Some(beats) => format!("{label} · {mode} · {beats:.2} beats"),
                None => format!("{label} · {mode}"),
            }
        });

        // Track rows: header widgets + clip canvas
        let mut track_rows = column![].spacing(0);
        let empty_content = TrackTimelineContent::default();

        for (track_index, track) in self.state.project_tracks.tracks.iter().enumerate() {
            let content = timeline.timeline.get(track.id).unwrap_or(&empty_content);
            let selected = timeline.selected_track == Some(track.id);
            let track_color = th::track_color(track.color_index);

            // Collect selected clip IDs for this track
            let selected_clips: HashSet<ClipId> = timeline
                .selected_clips
                .iter()
                .filter_map(|sel| match sel {
                    ArrangementSelection::AudioClip { track_id, clip_id }
                        if *track_id == track.id =>
                    {
                        Some(*clip_id)
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id }
                        if *track_id == track.id =>
                    {
                        Some(*clip_id)
                    }
                    _ => None,
                })
                .collect();

            // Track header (iced widgets)
            let editing = self.state.view.editing_track_name == Some(track.id);
            let automation_open = self.state.automation_ui.expanded.contains(&track.id);
            let header = view_track_header(
                track,
                selected,
                editing,
                &self.state.view.edit_name_text,
                automation_open,
            );

            // Clip canvas for this track
            let clip_canvas_widget = TrackClipCanvas::from_track(
                track,
                content,
                playhead_beats,
                zoom_level,
                self.state.view.grid_config(),
                scroll_offset,
                total_beats,
                sample_rate,
                selected,
                track_color,
                bpm,
                track.id,
                track_index,
                total_track_count,
                track_ids.clone(),
                track_kinds.clone(),
                selected_clips,
                self.state.transport.loop_enabled,
                self.state.transport.loop_start_beats,
                self.state.transport.loop_end_beats,
                timeline.time_selection_active,
                timeline.selection_start_beats,
                timeline.selection_end_beats,
                timeline.time_selection_track,
                self.state.browser.drag_source.is_some(),
                browser_drag_duration,
                browser_drag_detail.clone(),
            );
            let track_id = track.id;
            let compatible = !track.kind.is_midi();
            let grid = self.state.view.grid_config();
            let track_geometry = geometry;
            let clip_canvas: Element<'_, Message> = mouse_area(
                canvas(clip_canvas_widget)
                    .width(Length::Fill)
                    .height(Length::Fixed(70.0)),
            )
            .on_move(move |point| {
                let beat = grid.snap_beat(
                    track_geometry.x_to_beat(point.x),
                    track_geometry.pixels_per_beat(),
                );
                Message::Browser(BrowserMsg::DragHoverTrack {
                    track_id,
                    beat: beat.max(0.0),
                    compatible,
                })
            })
            .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
            .into();

            let track_row = row![header, clip_canvas].height(Length::Fixed(70.0));

            track_rows = track_rows.push(track_row);

            if automation_open {
                track_rows = self.push_automation_lanes(track_rows, timeline, track, track_color);
            }
        }

        if self.state.browser.drag_source.is_some() {
            let grid = self.state.view.grid_config();
            let empty_geometry = geometry;
            let empty_beat = match self.state.browser.drag_target {
                Some(crate::state::BrowserDropTarget::EmptyArrangement { beat }) => Some(beat),
                _ => None,
            };
            let label = empty_beat
                .map(|beat| format!("NEW AUDIO TRACK · BEAT {beat:.2}"))
                .unwrap_or_else(|| "NEW AUDIO TRACK".into());
            let header = container(text("+").size(14).color(th::accent()))
                .width(Length::Fixed(
                    crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
                ))
                .height(Length::Fixed(54.0))
                .align_x(iced::alignment::Horizontal::Center)
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
            let zone = mouse_area(
                container(text(label).size(12).color(th::accent()))
                    .padding([0, 10])
                    .width(Length::Fill)
                    .height(Length::Fixed(54.0))
                    .align_y(iced::alignment::Vertical::Center)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::accent_dim().into()),
                        border: iced::Border {
                            color: th::accent(),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .on_move(move |point| {
                let beat = grid.snap_beat(
                    empty_geometry.x_to_beat(point.x),
                    empty_geometry.pixels_per_beat(),
                );
                Message::Browser(BrowserMsg::DragHoverEmptyArrangement {
                    beat: beat.max(0.0),
                })
            })
            .on_release(Message::DropSampleOnEmptyArrangement);
            track_rows = track_rows.push(row![header, zone].height(Length::Fixed(54.0)));
        }

        // ── Returns + master: automation-only channels ──
        // Clipless lanes at the bottom, Ableton-style: a slim header
        // (select, expand, delete for returns) and their automation
        // lanes when expanded.
        let master_ref = &self.state.project_tracks.master;
        let channel_refs: Vec<&crate::state::ProjectTrack> = self
            .state
            .project_tracks
            .buses
            .iter()
            .chain(std::iter::once(master_ref))
            .collect();
        for channel in channel_refs {
            let is_master = channel.id.is_master();
            let chan_color = if is_master {
                th::accent()
            } else {
                th::track_color(channel.color_index)
            };
            let selected = timeline.selected_track == Some(channel.id);
            let expanded = self.state.automation_ui.expanded.contains(&channel.id);

            let toggle = button(
                icons::icon(icons::SLIDERS_VERTICAL)
                    .size(10)
                    .color(if expanded {
                        th::accent()
                    } else {
                        th::text_dim()
                    }),
            )
            .on_press(Message::Automation(
                crate::domains::automation::AutomationMsg::ToggleTrackLanes(channel.id),
            ))
            .padding([2, 4])
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: th::text_dim(),
                border: iced::Border::default(),
                ..Default::default()
            });

            let dot = text("\u{25CF}").size(9).color(chan_color);
            let name_color = if selected { th::text() } else { th::text_dim() };
            let name: Element<'_, Message> = if is_master {
                text(&channel.name).size(11).color(name_color).into()
            } else {
                view_editable_channel_name(
                    channel,
                    self.state.view.editing_track_name == Some(channel.id),
                    &self.state.view.edit_name_text,
                    11,
                    name_color,
                )
            };

            let remove_el: Element<'_, Message> = if is_master {
                text("").size(9).into()
            } else {
                button(icons::icon(icons::TRASH_2).size(9).color(th::text_dim()))
                    .on_press(Message::remove_bus(channel.id))
                    .padding([1, 4])
                    .style(|_theme: &Theme, status| {
                        let tc = match status {
                            button::Status::Hovered | button::Status::Pressed => th::danger(),
                            _ => th::text_dim(),
                        };
                        button::Style {
                            background: None,
                            text_color: tc,
                            border: iced::Border::default(),
                            ..Default::default()
                        }
                    })
                    .into()
            };
            let header_row = row![toggle, dot, name, horizontal_space(), remove_el]
                .spacing(6)
                .align_y(iced::Alignment::Center);

            let header: Element<'_, Message> = mouse_area(
                container(header_row)
                    .padding([0, 8])
                    .width(Length::Fixed(
                        crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
                    ))
                    .height(Length::Fixed(26.0))
                    .align_y(iced::alignment::Vertical::Center)
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(if selected {
                            th::track_bg_selected().into()
                        } else {
                            th::bg_surface().into()
                        }),
                        border: iced::Border {
                            color: if selected { chan_color } else { th::border() },
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .on_press(Message::select_track(channel.id))
            .into();

            let filler = container(column![])
                .width(Length::Fill)
                .height(Length::Fixed(26.0))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::divider(),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                });

            track_rows = track_rows.push(row![header, filler].height(Length::Fixed(26.0)));
            if expanded {
                track_rows = self.push_automation_lanes(track_rows, timeline, channel, chan_color);
            }
        }

        let content = column![ruler_row, minimap_row, track_rows];

        let scrollable_content = scrollable(content).direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ));

        // mouse_area only provides on_right_press (no cursor position),
        // so the right-click context menu from the scrollable background
        // opens at a default position. Track canvas right-clicks still
        // use the precise cursor location.
        mouse_area(
            container(scrollable_content)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    ..Default::default()
                }),
        )
        .on_right_press(Message::View(ViewMsg::ShowContextMenu {
            x: 400.0,
            y: 300.0,
            target: ContextMenuTarget::ArrangementEmpty,
        }))
        .into()
    }

    /// Lane strip under an expanded track: one row per lane plus the
    /// add-lane picker.
    pub(super) fn push_automation_lanes<'a>(
        &'a self,
        mut rows: iced::widget::Column<'a, Message>,
        timeline: &'a crate::state::TimelineEditorState,
        track: &'a crate::state::ProjectTrack,
        track_color: iced::Color,
    ) -> iced::widget::Column<'a, Message> {
        use crate::domains::automation::{target_label_with_buses, AutomationMsg};
        use crate::widgets::automation_lane::{AutomationLaneWidget, LANE_HEIGHT};

        let automation = timeline
            .timeline
            .get(track.id)
            .map(|content| content.automation.as_slice())
            .unwrap_or(&[]);
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
            let lane_header = container(
                row![
                    text(label).size(11).color(th::text_dim()),
                    horizontal_space(),
                    remove
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding([0, 10])
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(LANE_HEIGHT))
            .align_y(iced::alignment::Vertical::Center)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                ..Default::default()
            });

            let selected = match self.state.automation_ui.selected {
                Some((t, l, i)) if t == track.id && l == lane.id => Some(i),
                _ => None,
            };
            let (reference, min_label, max_label, ref_label) = lane_scale(track, &lane.target);
            let widget = AutomationLaneWidget {
                track_id: track.id,
                lane_id: lane.id,
                points: lane.points.clone(),
                color: track_color,
                zoom_level: self.state.view.zoom_level,
                scroll_offset_beats: self.state.view.scroll_offset_beats,
                grid: self.state.view.grid_config(),
                selected,
                reference,
                min_label,
                max_label,
                ref_label,
            };
            let lane_canvas: Element<'_, Message> = canvas(widget)
                .width(Length::Fill)
                .height(Length::Fixed(LANE_HEIGHT))
                .into();
            rows = rows.push(row![lane_header, lane_canvas].height(Length::Fixed(LANE_HEIGHT)));
        }

        // Add-lane entry: a button that opens a searchable picker
        // panel (plugins can expose hundreds of parameters).
        let picker_query = match &self.state.automation_ui.picker {
            Some((tid, q)) if *tid == track.id => Some(q.clone()),
            _ => None,
        };
        let picker_open = picker_query.is_some();

        let panel: Element<'_, Message> = if let Some(query) = picker_query {
            let mut choices: Vec<LaneChoice> = vec![
                LaneChoice {
                    label: "Volume".to_string(),
                    target: vibez_core::automation::AutomationTarget::TrackGain,
                },
                LaneChoice {
                    label: "Pan".to_string(),
                    target: vibez_core::automation::AutomationTarget::TrackPan,
                },
            ];
            if !track.plugin_instrument_descriptors.is_empty() {
                let name = track
                    .plugin_instrument_name
                    .clone()
                    .unwrap_or_else(|| "Plugin".to_string());
                for (param_index, d) in track.plugin_instrument_descriptors.iter().enumerate() {
                    choices.push(LaneChoice {
                        label: format!("{name}: {}", d.name),
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
                for (param_index, d) in vibez_instruments::descriptors_for(kind).iter().enumerate()
                {
                    choices.push(LaneChoice {
                        label: format!("{instrument_name}: {}", d.name),
                        target: vibez_core::automation::AutomationTarget::InstrumentParam {
                            param_index,
                        },
                    });
                }
            }
            for effect in &track.effects {
                for (param_index, d) in effect.descriptors.iter().enumerate() {
                    let effect_name = effect
                        .plugin_name
                        .clone()
                        .unwrap_or_else(|| format!("{:?}", effect.effect_type));
                    choices.push(LaneChoice {
                        label: format!("{effect_name}: {}", d.name),
                        target: vibez_core::automation::AutomationTarget::EffectParam {
                            effect_id: effect.id,
                            param_index,
                        },
                    });
                }
            }
            // Send lanes: regular tracks only (buses and the master
            // have no sends).
            let is_channel = track.id.is_master()
                || self
                    .state
                    .project_tracks
                    .buses
                    .iter()
                    .any(|b| b.id == track.id);
            if !is_channel {
                for bus in &self.state.project_tracks.buses {
                    choices.push(LaneChoice {
                        label: format!("Send: {}", bus.name),
                        target: vibez_core::automation::AutomationTarget::Send { bus_id: bus.id },
                    });
                }
            }
            choices.retain(|c| !automation.iter().any(|l| l.target == c.target));

            let needle = query.to_lowercase();
            let total_before = choices.len();
            if !needle.is_empty() {
                choices.retain(|c| c.label.to_lowercase().contains(&needle));
            }
            let shown = choices.len().min(MAX_PICKER_RESULTS);
            let hidden = choices.len() - shown;

            let search = iced::widget::text_input("Search parameters\u{2026}", &query)
                .on_input(|q| Message::Automation(AutomationMsg::LanePickerQuery(q)))
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
                let target = choice.target;
                let track_id = track.id;
                list = list.push(
                    button(text(choice.label).size(11).color(th::text()))
                        .on_press(Message::Automation(AutomationMsg::AddLane {
                            track_id,
                            target,
                        }))
                        .width(Length::Fill)
                        .padding([3, 10])
                        .style(|_theme: &Theme, status| {
                            let bg = match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::bg_hover().into())
                                }
                                _ => None,
                            };
                            button::Style {
                                background: bg,
                                text_color: th::text(),
                                border: iced::Border::default(),
                                ..Default::default()
                            }
                        }),
                );
            }
            if hidden > 0 {
                list = list.push(
                    container(
                        text(format!("{hidden} more \u{2014} refine the search"))
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
            .width(Length::Fill)
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
            let track_id = track.id;
            container(
                button(text("+ Add automation").size(11).color(th::text_dim()))
                    .on_press(Message::Automation(AutomationMsg::OpenLanePicker(track_id)))
                    .width(Length::Fill)
                    .padding([3, 10])
                    .style(|_theme: &Theme, status| {
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::bg_hover().into())
                            }
                            _ => Some(th::bg_elevated().into()),
                        };
                        button::Style {
                            background: bg,
                            text_color: th::text_dim(),
                            border: iced::Border {
                                color: th::border(),
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        }
                    }),
            )
            .padding([2, 10])
            .into()
        };

        // Collapsed: the button sits inside the header column like a
        // lane header. Open: the search panel widens over the lane
        // area like a dropdown.
        let panel_width = if picker_open {
            crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH + 300.0
        } else {
            crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH
        };
        let picker_row = row![
            container(panel)
                .width(Length::Fixed(panel_width))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_surface().into()),
                    ..Default::default()
                }),
            iced::widget::horizontal_space()
        ];
        rows.push(picker_row)
    }
}

const MAX_PICKER_RESULTS: usize = 40;

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
            let r = reference
                .map(|n| fmt_value(n * 2.0, ""))
                .unwrap_or_default();
            (reference, "0".into(), "2.0".into(), r)
        }
        AutomationTarget::TrackPan => {
            let r = match reference {
                Some(n) if (n - 0.5).abs() < 0.01 => "C".to_string(),
                Some(n) => fmt_value(n * 2.0 - 1.0, ""),
                None => String::new(),
            };
            (reference, "L".into(), "R".into(), r)
        }
        _ => match target_descriptor(target, track) {
            Some(d) => {
                let r = reference
                    .map(|n| fmt_value(d.min + n * (d.max - d.min), d.unit))
                    .unwrap_or_default();
                (
                    reference,
                    fmt_value(d.min, d.unit),
                    fmt_value(d.max, d.unit),
                    r,
                )
            }
            None => (reference, String::new(), String::new(), String::new()),
        },
    }
}

fn fmt_value(v: f32, unit: &str) -> String {
    let num = if v.abs() >= 1000.0 {
        format!("{:.0}k", v / 1000.0)
    } else if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if unit.is_empty() {
        num
    } else {
        format!("{num} {unit}")
    }
}

/// One option in the add-lane picker.
#[derive(Debug, Clone, PartialEq)]
struct LaneChoice {
    label: String,
    target: vibez_core::automation::AutomationTarget,
}

impl std::fmt::Display for LaneChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}
