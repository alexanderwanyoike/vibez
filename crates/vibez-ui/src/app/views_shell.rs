//! Split out of app.rs; inherent methods on [`super::App`].

use std::collections::HashSet;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable,
    stack, text, text_input,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, TrackId};

use crate::icons;
use crate::message::Message;
use crate::state::{AppState, ArrangementSelection, ContextMenuTarget, Workspace};
use crate::theme as th;
use crate::widgets::mixer_strip::view_mixer_strip;
use crate::widgets::timeline::{ArrangementMinimap, MinimapTrack, RulerWidget, TrackClipCanvas};
use crate::widgets::track_header::view_track_header;
use crate::widgets::vu_meter::VuMeterWidget;

use super::*;

impl App {
    // ── View ──

    pub(super) fn view(&self) -> Element<'_, Message> {
        let header = self.view_header();

        let workspace_content = match self.state.view.workspace {
            Workspace::Arrange => self.view_arrangement(),
            Workspace::Mix => self.view_mixer(),
        };
        let content: Element<'_, Message> =
            if self.state.view.workspace == Workspace::Arrange && self.state.browser.open {
                row![self.view_sample_browser_panel(), workspace_content]
                    .height(Length::FillPortion(5))
                    .into()
            } else {
                workspace_content
            };

        let detail_panel = self.view_detail_panel();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, transport_bar, content, detail_panel, status_bar];

        let layout_container = container(layout).width(Length::Fill).height(Length::Fill);
        // Outer mouse_area cancels an active sample-drag on any release
        // that wasn't captured by a drop target (clip canvas, drum pad).
        let base_layout: Element<'_, Message> = mouse_area(layout_container)
            .on_release(Message::Browser(BrowserMsg::EndDragSample))
            .into();

        if self.state.settings_open {
            stack![base_layout, self.view_settings_modal()].into()
        } else if self.state.project.file_menu_open {
            stack![base_layout, self.view_file_menu_overlay()].into()
        } else if self.state.view.context_menu.is_some() {
            stack![base_layout, self.view_context_menu_overlay()].into()
        } else if self.state.view.editing_clip_name.is_some() {
            stack![base_layout, self.view_rename_overlay()].into()
        } else if self.state.devices.context_menu.is_some() {
            stack![base_layout, self.view_device_context_menu_overlay()].into()
        } else {
            base_layout
        }
    }

    pub(super) fn view_header(&self) -> Element<'_, Message> {
        let title = text("vibez").size(22).color(th::ACCENT);

        // Workspace tabs
        let arrange_tab = {
            let active = self.state.view.workspace == Workspace::Arrange;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::LAYOUT_LIST).size(13).color(text_color),
                    text("Arrange").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::View(ViewMsg::SwitchWorkspace(Workspace::Arrange)))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let mix_tab = {
            let active = self.state.view.workspace == Workspace::Mix;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(13)
                        .color(text_color),
                    text("Mix").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::View(ViewMsg::SwitchWorkspace(Workspace::Mix)))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let tabs = row![arrange_tab, mix_tab].spacing(4);

        let file_btn = button(text("File").size(13).color(th::TEXT_DIM))
            .on_press(Message::Project(ProjectMsg::ToggleFileMenu))
            .padding([6, 14])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT_DIM,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let browser_active = self.state.browser.open;
        let browser_btn = button(
            row![
                icons::icon(icons::AUDIO_WAVEFORM)
                    .size(13)
                    .color(if browser_active {
                        th::ACCENT
                    } else {
                        th::TEXT_DIM
                    }),
                text("Browser").size(13).color(if browser_active {
                    th::ACCENT
                } else {
                    th::TEXT_DIM
                })
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Browser(BrowserMsg::ToggleSampleBrowser))
        .padding([6, 14])
        .style(move |_theme: &Theme, status| {
            let bg = if browser_active {
                Some(th::BG_ELEVATED.into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                }
            };
            button::Style {
                background: bg,
                text_color: if browser_active {
                    th::ACCENT
                } else {
                    th::TEXT_DIM
                },
                border: iced::Border {
                    color: if browser_active {
                        th::ACCENT_DIM
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if browser_active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        let header_row = row![title, file_btn, browser_btn, tabs, horizontal_space()].spacing(8);

        let header = header_row.padding(10).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Arrangement view ──

    pub(super) fn view_arrangement(&self) -> Element<'_, Message> {
        if self.state.arrangement.tracks.is_empty() {
            let prompt = text("Right-click or Ctrl+T to add a track")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return mouse_area(
                container(centered)
                    .width(Length::Fill)
                    .height(Length::FillPortion(5))
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::BG_DARK.into()),
                        ..Default::default()
                    }),
            )
            .on_right_press(Message::View(ViewMsg::ShowContextMenu {
                x: 400.0,
                y: 300.0,
                target: ContextMenuTarget::ArrangementEmpty,
            }))
            .into();
        }

        let playhead_beats = self.state.position_beats();
        let sample_rate = self.state.transport.sample_rate;
        let bpm = self.state.transport.bpm;
        let zoom_level = self.state.view.zoom_level;
        let scroll_offset = self.state.view.scroll_offset_beats;
        let total_beats = self.state.total_beats();

        // Beat-based ruler across the top (offset by track header width)
        let ruler = RulerWidget {
            playhead_beats,
            bpm,
            zoom_level,
            scroll_offset_beats: scroll_offset,
            total_beats,
            loop_enabled: self.state.transport.loop_enabled,
            loop_start_beats: self.state.transport.loop_start_beats,
            loop_end_beats: self.state.transport.loop_end_beats,
            time_selection_active: self.state.arrangement.time_selection_active,
            selection_start_beats: self.state.arrangement.selection_start_beats,
            selection_end_beats: self.state.arrangement.selection_end_beats,
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
                background: Some(crate::theme::BG_SURFACE.into()),
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
                .arrangement
                .tracks
                .iter()
                .map(|t| {
                    let color = th::track_color(t.color_index);
                    let mut clips: Vec<(f64, f64)> = t
                        .clips
                        .iter()
                        .map(|c| (c.position as f64 / spb, c.duration as f64 / spb))
                        .collect();
                    clips.extend(
                        t.note_clips
                            .iter()
                            .map(|c| (c.position_beats, c.duration_beats)),
                    );
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
                background: Some(th::BG_SURFACE.into()),
                ..Default::default()
            });
        let minimap_canvas: Element<'_, Message> = canvas(minimap)
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .into();
        let minimap_row = row![minimap_spacer, minimap_canvas];

        // Collect track IDs and kinds for cross-track drag
        let track_ids: Vec<TrackId> = self.state.arrangement.tracks.iter().map(|t| t.id).collect();
        let track_kinds: Vec<bool> = self
            .state
            .arrangement
            .tracks
            .iter()
            .map(|t| t.kind.is_midi())
            .collect();
        let total_track_count = self.state.arrangement.tracks.len();

        // Track rows: header widgets + clip canvas
        let mut track_rows = column![].spacing(0);

        for (track_index, track) in self.state.arrangement.tracks.iter().enumerate() {
            let selected = self.state.arrangement.selected_track == Some(track.id);
            let track_color = th::track_color(track.color_index);

            // Collect selected clip IDs for this track
            let selected_clips: HashSet<ClipId> = self
                .state
                .arrangement
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
                playhead_beats,
                zoom_level,
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
                self.state.arrangement.time_selection_active,
                self.state.arrangement.selection_start_beats,
                self.state.arrangement.selection_end_beats,
                self.state.arrangement.time_selection_track,
                self.state.browser.drag_source.is_some(),
            );
            let clip_canvas: Element<'_, Message> = canvas(clip_canvas_widget)
                .width(Length::Fill)
                .height(Length::Fixed(70.0))
                .into();

            let track_row = row![header, clip_canvas].height(Length::Fixed(70.0));

            track_rows = track_rows.push(track_row);

            if automation_open {
                track_rows = self.push_automation_lanes(track_rows, track, track_color);
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
                    background: Some(th::BG_DARK.into()),
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

    // ── Mixer view ──

    pub(super) fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.arrangement.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                })
                .into();
        }

        // ── Channel strips + pinned master ──
        let mut strips = row![].spacing(4).padding(8).height(Length::Fill);

        for track in &self.state.arrangement.tracks {
            let selected = self.state.arrangement.selected_track == Some(track.id);
            let strip = view_mixer_strip(track, selected);
            strips = strips.push(strip);
        }

        // Master strip — pinned to far right
        let master_label = text("Master").size(12).color(th::TEXT);
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(32.0))
            .height(Length::Fill)
            .into();

        let master_col = column![master_label, master_meter_canvas]
            .spacing(4)
            .padding(8)
            .width(Length::Fixed(100.0))
            .height(Length::Fill)
            .align_x(iced::Alignment::Center);

        let master_container =
            container(master_col)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });

        let mixer_row = row![strips, horizontal_space(), master_container]
            .spacing(4)
            .padding([8, 4])
            .height(Length::Fill);

        let mixer_content = container(mixer_row)
            .width(Length::Fill)
            .height(Length::Fill);

        mouse_area(
            container(mixer_content)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
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

    // ── Transport bar ──

    pub(super) fn view_transport(&self) -> Element<'_, Message> {
        // Skip back button
        let skip_back_btn = button(icons::icon(icons::SKIP_BACK).size(16).color(th::TEXT))
            .on_press(Message::Transport(TransportMsg::Stop))
            .padding([8, 12])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Play/Pause button
        let play_pause_btn = if self.state.transport.playing {
            button(icons::icon(icons::PAUSE).size(16).color(th::ACCENT))
                .on_press(Message::Transport(TransportMsg::Stop))
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::PLAY).size(16).color(th::SUCCESS))
                .on_press(Message::Transport(TransportMsg::Play))
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::SUCCESS,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        // Loop toggle button
        let loop_btn = if self.state.transport.loop_enabled {
            button(icons::icon(icons::REPEAT).size(16).color(th::ACCENT))
                .on_press(Message::Transport(TransportMsg::ToggleArrangementLoop))
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::REPEAT).size(16).color(th::TEXT_DIM))
                .on_press(Message::Transport(TransportMsg::ToggleArrangementLoop))
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        let transport_buttons = row![skip_back_btn, play_pause_btn, loop_btn].spacing(4);

        // Time display
        let time_text = text(format!(
            "{} / {}",
            AppState::format_time(self.state.position_seconds()),
            AppState::format_time(self.state.duration_seconds()),
        ))
        .size(14)
        .color(th::TEXT);

        // BPM
        let bpm_input = text_input("BPM", &self.state.transport.bpm_text)
            .on_input(|t| Message::Transport(TransportMsg::BpmChanged(t)))
            .on_submit(Message::Transport(TransportMsg::BpmSubmit))
            .width(Length::Fixed(55.0))
            .size(14);

        let bpm_nudge = |icon: char, delta: f64| {
            button(icons::icon(icon).size(8).color(th::TEXT_DIM))
                .on_press(Message::Transport(TransportMsg::NudgeBpm(delta)))
                .padding([0, 4])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT_DIM,
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
        };
        let bpm_spinner = column![
            bpm_nudge(icons::CHEVRON_UP, 1.0),
            bpm_nudge(icons::CHEVRON_DOWN, -1.0),
        ]
        .spacing(1);

        let bpm_label = text("BPM").size(12).color(th::TEXT_DIM);

        // Master VU meter
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(28.0))
            .into();

        let volume_icon = icons::icon(icons::VOLUME_2).size(14).color(th::TEXT_DIM);

        let transport = row![
            transport_buttons,
            horizontal_space(),
            time_text,
            horizontal_space(),
            volume_icon,
            master_meter_canvas,
            row![bpm_input, bpm_spinner]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            bpm_label,
        ]
        .spacing(12)
        .padding(10)
        .align_y(iced::Alignment::Center);

        container(transport)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_status(&self) -> Element<'_, Message> {
        let status = text(&self.state.status_text).size(11).color(th::TEXT_DIM);

        container(status)
            .width(Length::Fill)
            .padding([3, 12])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    /// Lane strip under an expanded track: one row per lane plus the
    /// add-lane picker.
    fn push_automation_lanes<'a>(
        &'a self,
        mut rows: iced::widget::Column<'a, Message>,
        track: &'a crate::state::UiTrack,
        track_color: iced::Color,
    ) -> iced::widget::Column<'a, Message> {
        use crate::domains::automation::{target_label, AutomationMsg};
        use crate::widgets::automation_lane::{AutomationLaneWidget, LANE_HEIGHT};

        for lane in &track.automation {
            let label = target_label(&lane.target, track);
            let remove = button(icons::icon(icons::TRASH_2).size(9).color(th::TEXT_DIM))
                .on_press(Message::Automation(AutomationMsg::RemoveLane {
                    track_id: track.id,
                    lane_id: lane.id,
                }))
                .padding([1, 4])
                .style(|_theme: &Theme, _status| button::Style {
                    background: None,
                    text_color: th::TEXT_DIM,
                    border: iced::Border::default(),
                    ..Default::default()
                });
            let lane_header = container(
                row![
                    text(label).size(11).color(th::TEXT_DIM),
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
                background: Some(th::BG_SURFACE.into()),
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
                snap: self.state.view.snap_grid,
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
            choices.retain(|c| !track.automation.iter().any(|l| l.target == c.target));

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
                    background: th::BG_DARK.into(),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    icon: th::TEXT_DIM,
                    placeholder: th::TEXT_DIM,
                    value: th::TEXT,
                    selection: th::ACCENT,
                });
            let close = button(icons::icon(icons::X).size(10).color(th::TEXT_DIM))
                .on_press(Message::Automation(AutomationMsg::CloseLanePicker))
                .padding([3, 6])
                .style(|_theme: &Theme, _status| button::Style {
                    background: None,
                    text_color: th::TEXT_DIM,
                    border: iced::Border::default(),
                    ..Default::default()
                });

            let mut list = column![].spacing(1);
            for choice in choices.into_iter().take(MAX_PICKER_RESULTS) {
                let target = choice.target;
                let track_id = track.id;
                list = list.push(
                    button(text(choice.label).size(11).color(th::TEXT))
                        .on_press(Message::Automation(AutomationMsg::AddLane {
                            track_id,
                            target,
                        }))
                        .width(Length::Fill)
                        .padding([3, 10])
                        .style(|_theme: &Theme, status| {
                            let bg = match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::BG_HOVER.into())
                                }
                                _ => None,
                            };
                            button::Style {
                                background: bg,
                                text_color: th::TEXT,
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
                            .color(th::TEXT_DIM),
                    )
                    .padding([3, 10]),
                );
            }
            if total_before == 0 {
                list = list.push(
                    container(
                        text("Everything is already automated")
                            .size(10)
                            .color(th::TEXT_DIM),
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
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
        } else {
            let track_id = track.id;
            container(
                button(text("+ Add automation").size(11).color(th::TEXT_DIM))
                    .on_press(Message::Automation(AutomationMsg::OpenLanePicker(track_id)))
                    .width(Length::Fill)
                    .padding([3, 10])
                    .style(|_theme: &Theme, status| {
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::BG_HOVER.into())
                            }
                            _ => Some(th::BG_ELEVATED.into()),
                        };
                        button::Style {
                            background: bg,
                            text_color: th::TEXT_DIM,
                            border: iced::Border {
                                color: th::BORDER,
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
                    background: Some(th::BG_SURFACE.into()),
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
    track: &crate::state::UiTrack,
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
