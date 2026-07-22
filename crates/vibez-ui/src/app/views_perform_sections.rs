//! Section Timeline Editor view for the Perform workspace.

use std::collections::HashSet;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable,
    stack, text,
};
use iced::{Element, Length, Theme};

use crate::domains::perform::{PerformEditorFocus, PerformMsg};
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::{automation::AutomationMsg, browser::BrowserMsg};
use crate::icons;
use crate::message::Message;
use crate::state::{ArrangementSelection, TrackTimelineContent};
use crate::theme as th;
use crate::typography::{PERFORM_DISPLAY, PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};
use crate::widgets::timeline::{TimelineNoteClip, TrackClipCanvas};

use super::views_automation::AutomationLaneLayout;
use super::views_perform::{SECTION_BAR_WIDTH, SECTION_TRACK_GUTTER_WIDTH};
use super::*;

impl App {
    pub(super) fn view_section_construction(&self, section_width: f32) -> Element<'_, Message> {
        let selected = self
            .state
            .perform
            .selected_section
            .and_then(|id| self.state.perform.sections.by_id(id));
        let toolbar = self.view_section_toolbar(selected, section_width);
        let editor = self.state.perform.section_editor.editor();
        let recording_preview = self.state.perform.section_record.live_preview();
        let bar_count = selected
            .map(|section| (section.length_beats / 4.0).round() as usize)
            .unwrap_or(4)
            .max(1);
        let total_beats = selected.map_or(16.0, |section| section.length_beats);
        let timeline_width = SECTION_BAR_WIDTH * bar_count as f32;
        let row_height = if self.state.perform.section_timeline_expanded {
            78.0
        } else {
            58.0
        };
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

        let timeline: Element<'_, Message> = if selected.is_none() {
            center(
                column![
                    text("SELECT A SECTION")
                        .font(PERFORM_LABEL)
                        .size(13)
                        .color(th::text()),
                    text("Create one from an empty Pad Position")
                        .size(10)
                        .color(th::text_dim()),
                    text("SECTION DATA IS SAVED WITH THE PROJECT")
                        .font(PERFORM_TECH)
                        .size(8)
                        .color(th::text_muted())
                ]
                .spacing(8)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if self.state.project_tracks.tracks.is_empty() {
            center(
                column![
                    text("NO PROJECT TRACKS")
                        .font(PERFORM_LABEL)
                        .size(12)
                        .color(th::text()),
                    text("Add a MIDI Project Track in Arrange, then return to author this Section")
                        .size(10)
                        .color(th::text_dim())
                ]
                .spacing(8)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            let selected_section = match selected {
                Some(section) => section,
                None => unreachable!("empty Section selection is rendered above"),
            };
            let section_id = selected_section.id;
            let ruler_gutter = container(
                column![
                    text("PROJECT TRACK")
                        .font(PERFORM_TECH)
                        .size(8)
                        .color(th::text_dim()),
                    text(if self.state.perform.section_timeline_expanded {
                        "EXPANDED TIMELINE"
                    } else {
                        "COMPACT OVERVIEW"
                    })
                    .font(PERFORM_TECH_STRONG)
                    .size(7)
                    .color(th::accent())
                ]
                .spacing(3),
            )
            .width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH))
            .height(Length::Fixed(32.0))
            .padding([6, 9])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::perform_grid_line(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            });

            let mut ruler_marks = row![]
                .width(Length::Fixed(timeline_width))
                .height(Length::Fixed(32.0));
            for bar in 0..bar_count {
                ruler_marks = ruler_marks.push(
                    container(
                        text((bar + 1).to_string())
                            .font(PERFORM_TECH_STRONG)
                            .size(10)
                            .color(th::text_dim()),
                    )
                    .width(Length::Fixed(SECTION_BAR_WIDTH))
                    .height(Length::Fixed(32.0))
                    .padding([8, 10])
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::bg_dark().into()),
                        border: iced::Border {
                            color: th::perform_grid_line(),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
                );
            }

            let track_ids: Vec<_> = self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| track.id)
                .collect();
            let track_kinds: Vec<_> = self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| track.kind.is_midi())
                .collect();
            let total_tracks = track_ids.len();
            let empty_content = TrackTimelineContent::default();
            let mut gutters = column![];
            let mut lanes = column![].width(Length::Fixed(timeline_width));
            let mut content_height = 32.0;

            for (index, track) in self.state.project_tracks.tracks.iter().enumerate() {
                let content = editor.timeline.get(track.id).unwrap_or(&empty_content);
                let selected_track = editor.selected_track == Some(track.id);
                let track_color = th::track_color(track.color_index);
                let clip_count = content.note_clips.len() + content.clips.len();
                let hidden_note_count = content
                    .note_clips
                    .iter()
                    .filter(|clip| !selected_section.contains_playable_beat(clip.position_beats))
                    .count();
                let hidden_audio_count = content
                    .clips
                    .iter()
                    .filter(|clip| {
                        let beat = clip.position as f64 * self.state.transport.bpm
                            / (self.state.transport.sample_rate.max(1) as f64 * 60.0);
                        !selected_section.contains_playable_beat(beat)
                    })
                    .count();
                let hidden_count = hidden_note_count + hidden_audio_count;
                let has_content = clip_count > 0 || !content.automation.is_empty();
                let track_id = track.id;
                let type_label = if track.kind.is_midi() {
                    "MIDI"
                } else {
                    "AUDIO"
                };
                let marker = container(horizontal_space())
                    .width(Length::Fixed(3.0))
                    .height(Length::Fixed(
                        if self.state.perform.section_timeline_expanded {
                            28.0
                        } else {
                            20.0
                        },
                    ))
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(track_color.into()),
                        ..Default::default()
                    });
                let identity = button(
                    row![
                        marker,
                        column![
                            text(track.name.clone())
                                .font(PERFORM_DISPLAY)
                                .size(11)
                                .color(if selected_track {
                                    th::text()
                                } else {
                                    th::text_dim()
                                }),
                            text(if hidden_count > 0 {
                                format!("{type_label} · {clip_count} CLIPS · {hidden_count} HIDDEN")
                            } else {
                                format!(
                                    "{type_label} · {clip_count} CLIP{}",
                                    if clip_count == 1 { "" } else { "S" }
                                )
                            })
                            .font(PERFORM_TECH)
                            .size(7)
                            .color(th::text_muted())
                        ]
                        .spacing(3)
                    ]
                    .spacing(7)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::select_track(track_id))
                .padding([2, 5])
                .width(Length::Fill)
                .style(move |_theme: &Theme, status| button::Style {
                    background: Some(
                        if selected_track || matches!(status, button::Status::Hovered) {
                            th::blend(th::bg_hover(), track_color, 0.12)
                        } else {
                            th::bg_surface()
                        }
                        .into(),
                    ),
                    text_color: th::text(),
                    border: iced::Border::default(),
                    ..Default::default()
                });
                let add_clip: Element<'_, Message> = if track.kind.is_midi() {
                    button(icons::icon(icons::PLUS).size(11).color(th::accent()))
                        .on_press(Message::PianoRoll(PianoRollMsg::AddNoteClipToTrack(
                            track_id,
                        )))
                        .padding([5, 7])
                        .style(|_theme: &Theme, status| button::Style {
                            background: Some(
                                if matches!(status, button::Status::Hovered) {
                                    th::bg_hover()
                                } else {
                                    th::perform_inset()
                                }
                                .into(),
                            ),
                            text_color: th::accent(),
                            border: iced::Border {
                                color: th::border_light(),
                                width: 1.0,
                                radius: 2.0.into(),
                            },
                            ..Default::default()
                        })
                        .into()
                } else {
                    container(
                        icons::icon(icons::AUDIO_WAVEFORM)
                            .size(10)
                            .color(th::text_dim()),
                    )
                    .padding([5, 7])
                    .into()
                };
                let automation_open = self.state.automation_ui.expanded.contains(&track.id);
                let automation_toggle = button(
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(10)
                        .color(if automation_open {
                            th::accent()
                        } else {
                            th::text_dim()
                        }),
                )
                .on_press(Message::Automation(AutomationMsg::ToggleTrackLanes(
                    track_id,
                )))
                .padding([2, 5])
                .style(|_theme: &Theme, status| button::Style {
                    background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                        .then(|| th::bg_hover().into()),
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                });
                let remove_content: Element<'_, Message> = if has_content {
                    button(icons::icon(icons::X).size(9).color(th::text_dim()))
                        .on_press(Message::Perform(PerformMsg::RemoveTrackContent {
                            section_id,
                            track_id,
                        }))
                        .padding([2, 5])
                        .style(|_theme: &Theme, status| button::Style {
                            background: matches!(
                                status,
                                button::Status::Hovered | button::Status::Pressed
                            )
                            .then(|| th::with_alpha(th::danger(), 0.16).into()),
                            text_color: th::text_dim(),
                            border: iced::Border::default(),
                            ..Default::default()
                        })
                        .into()
                } else {
                    horizontal_space().width(Length::Shrink).into()
                };
                let controls = row![
                    horizontal_space(),
                    add_clip,
                    automation_toggle,
                    remove_content
                ]
                .spacing(1)
                .align_y(iced::Alignment::Center);
                let gutter = container(column![identity, controls].spacing(0))
                    .width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH))
                    .height(Length::Fixed(row_height))
                    .padding([2, 3])
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(th::bg_surface().into()),
                        border: iced::Border {
                            color: if selected_track {
                                th::blend(th::accent_dim(), track_color, 0.35)
                            } else {
                                th::perform_grid_line()
                            },
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    });

                let selected_clips: HashSet<_> = editor
                    .selected_clips
                    .iter()
                    .filter_map(|selection| match selection {
                        ArrangementSelection::AudioClip { track_id, clip_id }
                        | ArrangementSelection::NoteClip { track_id, clip_id }
                            if *track_id == track.id =>
                        {
                            Some(*clip_id)
                        }
                        _ => None,
                    })
                    .collect();
                let mut clip_canvas = TrackClipCanvas::from_track(
                    track,
                    content,
                    -1.0,
                    2.0,
                    self.state.view.grid_config(),
                    0.0,
                    total_beats,
                    self.state.transport.sample_rate,
                    selected_track,
                    track_color,
                    self.state.transport.bpm,
                    track.id,
                    index,
                    total_tracks,
                    track_ids.clone(),
                    track_kinds.clone(),
                    selected_clips,
                    false,
                    0.0,
                    total_beats,
                    editor.time_selection_active,
                    editor.selection_start_beats,
                    editor.selection_end_beats,
                    editor.time_selection_track,
                    self.state.browser.drag_source.is_some(),
                    browser_drag_duration,
                    browser_drag_detail.clone(),
                );
                if let Some(preview) = recording_preview.as_ref().filter(|preview| {
                    preview.section_id == section_id && preview.track_id == track.id
                }) {
                    clip_canvas = clip_canvas.with_recording_preview(TimelineNoteClip {
                        clip_id: preview.clip_id,
                        position_beats: preview.position_beats,
                        duration_beats: preview.length_beats,
                        name: "● RECORDING LIVE".into(),
                        notes: preview
                            .notes
                            .iter()
                            .map(|note| {
                                (
                                    note.pitch,
                                    (note.start_beat - preview.position_beats).max(0.0),
                                    note.duration_beats,
                                )
                            })
                            .collect(),
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                }
                let geometry = crate::timeline_geometry::TimelineGeometry::from_zoom(2.0, 0.0);
                let grid = self.state.view.grid_config();
                let compatible = !track.kind.is_midi();
                gutters = gutters.push(gutter);
                let clip_lane: Element<'_, Message> = if self.state.browser.drag_source.is_some() {
                    mouse_area(
                        canvas(clip_canvas)
                            .width(Length::Fixed(timeline_width))
                            .height(Length::Fixed(row_height)),
                    )
                    .on_move(move |point| {
                        Message::Browser(BrowserMsg::DragHoverTrack {
                            track_id,
                            beat: grid
                                .snap_beat(geometry.x_to_beat(point.x), geometry.pixels_per_beat())
                                .max(0.0),
                            compatible,
                        })
                    })
                    .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
                    .into()
                } else {
                    canvas(clip_canvas)
                        .width(Length::Fixed(timeline_width))
                        .height(Length::Fixed(row_height))
                        .into()
                };
                lanes = lanes.push(clip_lane);
                content_height += row_height;

                if automation_open {
                    let layout = AutomationLaneLayout {
                        header_width: SECTION_TRACK_GUTTER_WIDTH,
                        body_width: Some(timeline_width),
                        zoom_level: 2.0,
                        scroll_offset_beats: 0.0,
                    };
                    for part in
                        self.automation_lane_parts(track, &content.automation, track_color, layout)
                    {
                        gutters = gutters.push(part.header);
                        lanes = lanes.push(part.body);
                        content_height += part.height;
                    }
                }
            }

            let fixed_gutter =
                column![ruler_gutter, gutters].width(Length::Fixed(SECTION_TRACK_GUTTER_WIDTH));
            let timeline_base: Element<'_, Message> = container(column![ruler_marks, lanes])
                .width(Length::Fixed(timeline_width))
                .height(Length::Fixed(content_height))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::display_bg().into()),
                    ..Default::default()
                })
                .into();
            let timeline_content: Element<'_, Message> =
                if self.state.perform.playing_section == Some(section_id) {
                    let fraction = super::views_perform_playhead::section_playhead_fraction(
                        self.state.perform.section_playhead_samples,
                        selected_section.length_beats,
                        self.state.transport.bpm,
                        self.state.transport.sample_rate,
                    );
                    let playhead = row![
                        horizontal_space().width(Length::Fixed(timeline_width * fraction)),
                        super::views_perform_playhead::section_playhead_line(Length::Fixed(
                            content_height,
                        )),
                    ]
                    .width(Length::Fixed(timeline_width))
                    .height(Length::Fixed(content_height));
                    stack![timeline_base, playhead]
                        .width(Length::Fixed(timeline_width))
                        .height(Length::Fixed(content_height))
                        .into()
                } else {
                    timeline_base
                };
            let scrolling_timeline = scrollable::Scrollable::with_direction(
                timeline_content,
                scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new()
                        .width(5)
                        .scroller_width(5)
                        .spacing(1),
                ),
            )
            .width(Length::Fill)
            .height(Length::Fixed(content_height));
            scrollable::Scrollable::with_direction(
                row![fixed_gutter, scrolling_timeline]
                    .width(Length::Fill)
                    .height(Length::Fixed(content_height)),
                scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new()
                        .width(5)
                        .scroller_width(5)
                        .spacing(1),
                ),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        let construction = container(column![toolbar, timeline].height(Length::Fill))
            .width(Length::Fill)
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
