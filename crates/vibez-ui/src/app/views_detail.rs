//! Split out of app.rs; inherent methods on [`super::App`].

use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, pick_list, row, text, text_input,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, SectionId, TrackId};

use crate::icons;
use crate::message::Message;
use crate::state::{ArrangementSelection, DetailPanelTab, TimelineEditorState, UiClip};
use crate::theme as th;
use crate::widgets::audio_clip_detail::AudioClipDetailWidget;
use crate::widgets::piano_roll::{PianoRollWidget, VelocityLaneWidget};

use super::*;

const DETAIL_PANEL_MIN_HEIGHT: f32 = 180.0;
const MIDI_DETAIL_PANEL_MIN_HEIGHT: f32 = 360.0;
const SHELL_AND_WORKSPACE_MIN_HEIGHT: f32 = 360.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;

fn resolved_detail_playhead_samples(
    editing_perform: bool,
    selected_section: Option<SectionId>,
    playing_section: Option<SectionId>,
    arrange_samples: u64,
    section_samples: u64,
) -> Option<u64> {
    if !editing_perform {
        return Some(arrange_samples);
    }
    selected_section
        .filter(|selected| Some(*selected) == playing_section)
        .map(|_| section_samples)
}

fn effective_detail_panel_height(
    preferred_height: f32,
    window_height: f32,
    midi_clip_editor_visible: bool,
) -> f32 {
    let maximum = (window_height - SHELL_AND_WORKSPACE_MIN_HEIGHT).max(DETAIL_PANEL_MIN_HEIGHT);
    let preferred_height = if midi_clip_editor_visible {
        preferred_height.max(MIDI_DETAIL_PANEL_MIN_HEIGHT)
    } else {
        preferred_height
    };
    preferred_height.clamp(DETAIL_PANEL_MIN_HEIGHT, maximum)
}

fn visible_note_clip_for_track(editor: &TimelineEditorState, track_id: TrackId) -> Option<ClipId> {
    editor
        .selected_note_clip
        .filter(|(selected_track, _)| *selected_track == track_id)
        .map(|(_, clip_id)| clip_id)
        .or_else(|| {
            editor.selected_clips.iter().find_map(|selection| {
                if let ArrangementSelection::NoteClip {
                    track_id: selected_track,
                    clip_id,
                } = selection
                {
                    (*selected_track == track_id).then_some(*clip_id)
                } else {
                    None
                }
            })
        })
}

fn focused_note_clip_for_track(editor: &TimelineEditorState, track_id: TrackId) -> Option<ClipId> {
    editor
        .selected_note_clip
        .filter(|(selected_track, _)| *selected_track == track_id)
        .map(|(_, clip_id)| clip_id)
}

impl App {
    // ── Detail panel (Ableton-style device chain) ──

    /// The selected MIDI track that can host the piano-roll detail view.
    ///
    /// Visibility and keyboard focus intentionally share this structural
    /// gate, then resolve their clip from different selection state.
    fn piano_roll_track(&self) -> Option<TrackId> {
        if self.state.view.detail_panel_tab != DetailPanelTab::Clip {
            return None;
        }

        let editor = self.state.active_timeline_editor();
        let track_id = editor.selected_track?;
        self.state
            .find_track(track_id)
            .is_some_and(|track| track.kind.is_midi())
            .then_some(track_id)
    }

    /// A note clip the piano roll can render. Arrangement selection may
    /// supply the clip without transferring keyboard focus into the editor.
    pub(super) fn visible_piano_roll_clip(&self) -> Option<(TrackId, ClipId)> {
        let track_id = self.piano_roll_track()?;
        let clip_id = visible_note_clip_for_track(self.state.active_timeline_editor(), track_id)?;
        Some((track_id, clip_id))
    }

    /// The explicitly opened note clip that owns editor shortcuts.
    pub(super) fn focused_piano_roll_clip(&self) -> Option<(TrackId, ClipId)> {
        let track_id = self.piano_roll_track()?;
        let clip_id = focused_note_clip_for_track(self.state.active_timeline_editor(), track_id)?;
        Some((track_id, clip_id))
    }

    fn midi_clip_editor_visible(&self) -> bool {
        self.visible_piano_roll_clip().is_some()
    }

    pub(super) fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<'_, Message> = if let Some(track) = self
            .state
            .active_timeline_editor()
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;
            let track_color = th::track_color(track.color_index);

            // Tab bar
            let clip_tab = {
                let active = self.state.view.detail_panel_tab == DetailPanelTab::Clip;
                let (bg, text_color, border_color) = if active {
                    (th::bg_elevated(), th::accent(), th::accent_dim())
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::text_dim(),
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Clip").size(12).color(text_color))
                    .on_press(Message::View(ViewMsg::SwitchDetailTab(
                        DetailPanelTab::Clip,
                    )))
                    .padding([4, 12])
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
            let devices_tab = {
                let active = self.state.view.detail_panel_tab == DetailPanelTab::Devices;
                let (bg, text_color, border_color) = if active {
                    (th::bg_elevated(), th::accent(), th::accent_dim())
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::text_dim(),
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Devices").size(12).color(text_color))
                    .on_press(Message::View(ViewMsg::SwitchDetailTab(
                        DetailPanelTab::Devices,
                    )))
                    .padding([4, 12])
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
            let tab_bar = row![clip_tab, devices_tab].spacing(4).padding([4, 8]);

            // Tab content
            let tab_content: Element<'_, Message> = match self.state.view.detail_panel_tab {
                DetailPanelTab::Clip => {
                    let is_midi = track.kind.is_midi();

                    if self.visible_piano_roll_clip().is_some() {
                        self.view_piano_roll_panel(track_id, track_color)
                    } else if is_midi {
                        self.view_midi_track_clip_placeholder(track_id, track_color)
                    } else {
                        // Find a single selected audio clip on this track
                        let audio_sel = self
                            .state
                            .active_timeline_editor()
                            .selected_clips
                            .iter()
                            .find_map(|s| match s {
                                ArrangementSelection::AudioClip {
                                    track_id: tid,
                                    clip_id: cid,
                                } if *tid == track_id => Some(*cid),
                                _ => None,
                            });
                        if let Some(sel_cid) = audio_sel {
                            if let Some(clip) = self
                                .state
                                .active_timeline_content(track_id)
                                .and_then(|content| content.clips.iter().find(|c| c.id == sel_cid))
                            {
                                self.view_audio_clip_panel(track_id, clip, track_color)
                            } else {
                                self.view_clip_placeholder()
                            }
                        } else {
                            self.view_clip_placeholder()
                        }
                    }
                }
                DetailPanelTab::Devices => self.view_device_chain(track_id, track, track_color),
            };

            column![tab_bar, tab_content].height(Length::Fill).into()
        } else {
            let label = text("Select a track to view devices")
                .size(14)
                .color(th::text_dim());
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        let panel_height = effective_detail_panel_height(
            self.state.view.detail_panel_height,
            self.state.view.window_height,
            self.midi_clip_editor_visible(),
        );
        container(detail_content)
            .width(Length::Fill)
            .height(Length::Fixed(panel_height))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn detail_panel_drag_height(&self, cursor_y: f32) -> f32 {
        effective_detail_panel_height(
            self.state.view.window_height - cursor_y - STATUS_BAR_HEIGHT,
            self.state.view.window_height,
            self.midi_clip_editor_visible(),
        )
    }

    pub(super) fn view_clip_placeholder(&self) -> Element<'_, Message> {
        let label = text("Select a clip to view details")
            .size(14)
            .color(th::text_dim());
        center(label)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Piano roll panel for the detail panel split view.
    pub(super) fn view_piano_roll_panel(
        &self,
        track_id: TrackId,
        track_color: Color,
    ) -> Element<'_, Message> {
        use crate::state::PianoRollEditMode;

        let playhead_beats = resolved_detail_playhead_samples(
            self.state.view.workspace == crate::state::Workspace::Perform,
            self.state.perform.selected_section,
            self.state.perform.playing_section,
            self.state.transport.position_samples,
            self.state.perform.section_playhead_samples,
        )
        .map(|samples| {
            samples as f64 * self.state.transport.bpm
                / (f64::from(self.state.transport.sample_rate.max(1)) * 60.0)
        })
        .unwrap_or(-1.0);

        let visible_clip = self
            .visible_piano_roll_clip()
            .filter(|(open_track_id, _)| *open_track_id == track_id)
            .and_then(|(_, clip_id)| {
                let content = self.state.active_timeline_content(track_id)?;
                content.note_clips.iter().find(|clip| clip.id == clip_id)
            });

        let (piano_widget, velocity_widget) = match visible_clip {
            Some(clip) => {
                let clip_relative_playhead = playhead_beats - clip.position_beats;
                (
                    PianoRollWidget::from_clip(
                        track_id,
                        clip,
                        clip_relative_playhead,
                        clip.duration_beats,
                        track_color,
                        self.state.view.grid_config(),
                        self.state.piano_roll.scroll_y,
                        self.state.piano_roll.edit_mode,
                    ),
                    VelocityLaneWidget::from_clip(
                        track_id,
                        clip,
                        clip.duration_beats,
                        track_color,
                        self.state.view.grid_config(),
                    ),
                )
            }
            None => (
                PianoRollWidget::empty(track_id, playhead_beats, track_color),
                VelocityLaneWidget::empty(track_id, track_color),
            ),
        };

        let piano_canvas: Element<'_, Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let velocity_canvas: Element<'_, Message> = canvas(velocity_widget)
            .width(Length::Fill)
            .height(Length::Fixed(92.0))
            .into();

        // ── Clip properties bar (shown when a clip is selected) ──
        let mut content_col = column![].spacing(2).padding(4);

        if let Some(clip) = visible_clip {
            let clip_id = clip.id;
            let clip_loop = clip.loop_enabled;
            let groove_grid = clip.groove_grid;
            let clip_name = text(clip.name.clone()).size(11).color(th::text());
            let pos_label = text(format!("Pos: {:.1}", clip.position_beats))
                .size(10)
                .color(th::text_dim());
            let dur_label = text(format!("Dur: {:.1}", clip.duration_beats))
                .size(10)
                .color(th::text_dim());

            let swing_relationship = self.view_clip_swing_relationship(
                track_id,
                track_color,
                Some((clip_id, groove_grid)),
            );

            // Loop toggle
            let loop_icon_color = if clip_loop {
                th::accent()
            } else {
                th::text_dim()
            };
            let loop_btn = button(icons::icon(icons::REPEAT).size(10).color(loop_icon_color))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleNoteClipLoop(
                    track_id, clip_id,
                )))
                .padding([2, 4])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: if clip_loop {
                        Some(th::accent_dim().into())
                    } else {
                        Some(th::bg_elevated().into())
                    },
                    text_color: loop_icon_color,
                    border: iced::Border {
                        color: if clip_loop {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

            // Clip operation buttons
            let op_btn_style = |_theme: &Theme, _status| button::Style {
                background: Some(th::bg_elevated().into()),
                text_color: th::text_dim(),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            };

            let dup_btn = button(
                row![
                    icons::icon(icons::COPY).size(10).color(th::text_dim()),
                    text("Dup").size(10).color(th::text_dim())
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::duplicate_note_clip(track_id, clip_id))
            .padding([2, 6])
            .style(op_btn_style);

            let double_btn = button(text("2x").size(10).color(th::text_dim()))
                .on_press(Message::PianoRoll(PianoRollMsg::DoubleNoteClip(
                    track_id, clip_id,
                )))
                .padding([2, 6])
                .style(op_btn_style);

            let halve_btn = button(text("\u{00BD}x").size(10).color(th::text_dim()))
                .on_press(Message::PianoRoll(PianoRollMsg::HalveNoteClip(
                    track_id, clip_id,
                )))
                .padding([2, 6])
                .style(op_btn_style);

            let crop_btn = button(
                row![
                    icons::icon(icons::SCISSORS).size(10).color(th::text_dim()),
                    text("Crop").size(10).color(th::text_dim())
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::PianoRoll(PianoRollMsg::CropNoteClip(
                track_id, clip_id,
            )))
            .padding([2, 6])
            .style(op_btn_style);

            let props_row = row![
                clip_name,
                swing_relationship,
                horizontal_space(),
                pos_label,
                dur_label,
                loop_btn,
                dup_btn,
                double_btn,
                halve_btn,
                crop_btn,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);

            content_col = content_col.push(props_row);
        }

        // ── Header row: label, edit mode toggle, snap grid ──
        let label = text("Piano Roll").size(11).color(th::text_dim());

        // Edit mode toggle: Select / Draw
        let select_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Select;
        let draw_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Draw;

        let select_btn = {
            let (bg, tc) = if select_active {
                (th::accent_dim(), th::accent())
            } else {
                (th::bg_elevated(), th::text_dim())
            };
            button(icons::icon(icons::MOUSE_POINTER).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if select_active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let draw_btn = {
            let (bg, tc) = if draw_active {
                (th::accent_dim(), th::accent())
            } else {
                (th::bg_elevated(), th::text_dim())
            };
            button(icons::icon(icons::PENCIL).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if draw_active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let mode_row = row![select_btn, draw_btn].spacing(1);

        let snap_picker = pick_list(
            crate::state::SnapGrid::all(),
            Some(
                self.state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat()),
            ),
            |grid| Message::View(ViewMsg::SetSnapGrid(grid)),
        )
        .width(Length::Fixed(90.0));
        let snap_label = text("Snap:").size(10).color(th::text_dim());
        let header_row = row![label, mode_row, horizontal_space(), snap_label, snap_picker]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        content_col = content_col
            .push(header_row)
            .push(piano_canvas)
            .push(velocity_canvas);

        container(content_col.height(Length::Fill))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Audio clip waveform panel for the detail panel split view.
    pub(super) fn view_audio_clip_panel(
        &self,
        track_id: TrackId,
        clip: &UiClip,
        track_color: Color,
    ) -> Element<'_, Message> {
        let playhead_samples = resolved_detail_playhead_samples(
            self.state.view.workspace == crate::state::Workspace::Perform,
            self.state.perform.selected_section,
            self.state.perform.playing_section,
            self.state.transport.position_samples,
            self.state.perform.section_playhead_samples,
        );
        let playhead_normalized = playhead_samples
            .filter(|playhead| {
                clip.duration > 0
                    && *playhead >= clip.position
                    && *playhead < clip.position + clip.duration
            })
            .map(|playhead| (playhead - clip.position) as f64 / clip.duration as f64)
            .unwrap_or(-1.0);

        let waveform_widget = AudioClipDetailWidget {
            audio: Arc::clone(&clip.audio),
            duration_samples: clip.duration,
            source_offset: clip.source_offset,
            sample_rate: self.state.transport.sample_rate,
            track_color,
            playhead_normalized,
            loop_enabled: clip.loop_enabled,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
        };

        let waveform_canvas: Element<'_, Message> = canvas(waveform_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let label = text("Waveform").size(11).color(th::text_dim());
        let clip_info = text(format!(
            "{}: {:.1}s",
            clip.name,
            clip.duration as f64 / self.state.transport.sample_rate as f64
        ))
        .size(10)
        .color(th::text_muted());

        let header_row = row![label, horizontal_space(), clip_info]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let quantize_row = self.view_audio_quantize_row(track_id, clip.id);
        let warp_row = self.view_audio_warp_row(track_id, clip);

        let content = column![header_row, quantize_row, warp_row, waveform_canvas]
            .spacing(6)
            .padding(4);

        container(content)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_audio_warp_row(
        &self,
        track_id: TrackId,
        clip: &UiClip,
    ) -> Element<'_, Message> {
        let clip_id = clip.id;
        let label = text("Warp").size(11).color(th::text_dim());

        let default_text = clip
            .original_bpm
            .map(|bpm| format!("{:.1}", bpm))
            .unwrap_or_default();
        let text_value = self
            .state
            .arrangement
            .clip_bpm_edit
            .get(&clip_id)
            .cloned()
            .unwrap_or(default_text);

        let bpm_input = text_input("BPM", &text_value)
            .on_input(move |t| {
                Message::Arrangement(ArrangementMsg::ClipBpmInputChanged {
                    track_id,
                    clip_id,
                    text: t,
                })
            })
            .on_submit(Message::Arrangement(ArrangementMsg::SubmitClipBpm {
                track_id,
                clip_id,
            }))
            .size(11)
            .width(Length::Fixed(70.0));

        let button_style = |_theme: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => Some(th::bg_elevated().into()),
            };
            button::Style {
                background: bg,
                text_color: th::text(),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        };

        let detect_btn = button(text("Detect").size(11).color(th::text()))
            .on_press(Message::DetectClipBpm { track_id, clip_id })
            .padding([4, 10])
            .style(button_style);

        let warp_btn = button(
            text(format!("Warp → {:.0} BPM", self.state.transport.bpm))
                .size(11)
                .color(th::text()),
        )
        .on_press(Message::WarpClipToProject { track_id, clip_id })
        .padding([4, 10])
        .style(button_style);

        let mut row_widgets = row![label, bpm_input, detect_btn, warp_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        if clip.warped {
            let clear_btn = button(text("Clear warp").size(11).color(th::text_dim()))
                .on_press(Message::Arrangement(ArrangementMsg::ClearClipWarp {
                    track_id,
                    clip_id,
                }))
                .padding([4, 10])
                .style(button_style);
            row_widgets = row_widgets.push(clear_btn);

            if let Some(warped_to) = clip.warped_to_bpm {
                let stale = (warped_to - self.state.transport.bpm).abs() > 0.01;
                if stale {
                    row_widgets = row_widgets.push(
                        text(format!("(was {:.0})", warped_to))
                            .size(10)
                            .color(th::meter_yellow()),
                    );
                }
            }
        } else if let Some(detected) = clip.original_bpm {
            // Auto-warp declined (low confidence) or was never run:
            // the clip knows its tempo and it disagrees with the
            // project. Say so loudly instead of a status-bar whisper;
            // the Warp button on this same row is the one-click fix.
            if (detected - self.state.transport.bpm).abs() > 0.5 {
                row_widgets = row_widgets.push(
                    text(format!(
                        "OUT OF TEMPO: clip {detected:.1} BPM vs project {:.0}",
                        self.state.transport.bpm
                    ))
                    .size(10)
                    .color(th::meter_yellow()),
                );
            }
        }

        row_widgets.into()
    }

    pub(super) fn view_audio_quantize_row(
        &self,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Element<'_, Message> {
        let label = text("Quantize").size(11).color(th::text_dim());
        let grid_btn = |grid: crate::state::SnapGrid| -> Element<'_, Message> {
            button(text(grid.label()).size(11).color(th::text()))
                .on_press(Message::QuantizeAudioClipAt {
                    track_id,
                    clip_id,
                    grid,
                })
                .padding([4, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => Some(th::bg_elevated().into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .into()
        };

        row![
            label,
            grid_btn(crate::state::SnapGrid::QUARTER),
            grid_btn(crate::state::SnapGrid::EIGHTH),
            grid_btn(crate::state::SnapGrid::SIXTEENTH),
            grid_btn(crate::state::SnapGrid::THIRTY_SECOND),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_detail_panel_height, focused_note_clip_for_track,
        resolved_detail_playhead_samples, visible_note_clip_for_track,
    };
    use crate::state::{ArrangementSelection, TimelineEditorState};
    use vibez_core::id::{ClipId, SectionId, TrackId};

    #[test]
    fn detail_panel_height_preserves_the_workspace_at_small_windows() {
        assert_eq!(effective_detail_panel_height(80.0, 900.0, false), 180.0);
        assert_eq!(effective_detail_panel_height(360.0, 900.0, false), 360.0);
        assert_eq!(effective_detail_panel_height(800.0, 900.0, false), 540.0);
        assert_eq!(effective_detail_panel_height(320.0, 520.0, false), 180.0);
    }

    #[test]
    fn visible_midi_clip_keeps_both_note_and_velocity_editors_usable() {
        assert_eq!(effective_detail_panel_height(280.0, 900.0, true), 360.0);
        assert_eq!(effective_detail_panel_height(420.0, 900.0, true), 420.0);
        assert_eq!(effective_detail_panel_height(280.0, 520.0, true), 180.0);
    }

    #[test]
    fn detail_playhead_resolves_arrange_and_section_clocks_without_crossing_targets() {
        let playing = SectionId::new();
        let other = SectionId::new();

        assert_eq!(
            resolved_detail_playhead_samples(false, None, None, 96_000, 12_000),
            Some(96_000)
        );
        assert_eq!(
            resolved_detail_playhead_samples(true, Some(playing), Some(playing), 96_000, 12_000,),
            Some(12_000)
        );
        assert_eq!(
            resolved_detail_playhead_samples(true, Some(other), Some(playing), 96_000, 12_000,),
            None
        );
    }

    #[test]
    fn marquee_selected_note_clip_is_the_visible_piano_roll_clip() {
        let track_id = TrackId::new();
        let other_track = TrackId::new();
        let marquee_clip = ClipId::new();
        let explicit_clip = ClipId::new();
        let mut editor = TimelineEditorState::default();
        editor
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: marquee_clip,
            });

        assert_eq!(
            visible_note_clip_for_track(&editor, track_id),
            Some(marquee_clip)
        );
        assert_eq!(visible_note_clip_for_track(&editor, other_track), None);

        editor.selected_note_clip = Some((track_id, explicit_clip));
        assert_eq!(
            visible_note_clip_for_track(&editor, track_id),
            Some(explicit_clip)
        );
    }

    #[test]
    fn arrangement_selection_does_not_focus_the_piano_roll_for_select_all() {
        let track_id = TrackId::new();
        let selected_clip = ClipId::new();
        let explicitly_open_clip = ClipId::new();
        let mut editor = TimelineEditorState::default();
        editor
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: selected_clip,
            });

        // This is the state after the first arrangement Command+A. A
        // second press must remain arrangement select-all.
        assert_eq!(focused_note_clip_for_track(&editor, track_id), None);

        editor.selected_note_clip = Some((track_id, explicitly_open_clip));
        assert_eq!(
            focused_note_clip_for_track(&editor, track_id),
            Some(explicitly_open_clip)
        );
    }
}
