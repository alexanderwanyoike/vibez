//! Split out of app.rs; inherent methods on [`super::App`].

use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, row, text, text_input,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, TrackId};

use crate::icons;
use crate::message::Message;
use crate::state::{ArrangementSelection, DetailPanelTab, UiClip};
use crate::theme as th;
use crate::widgets::audio_clip_detail::AudioClipDetailWidget;
use crate::widgets::piano_roll::PianoRollWidget;

use super::*;

impl App {
    // ── Detail panel (Ableton-style device chain) ──

    pub(super) fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<'_, Message> = if let Some(track) = self
            .state
            .arrangement
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;
            let track_color = th::track_color(track.color_index);

            // Tab bar
            let clip_tab = {
                let active = self.state.view.detail_panel_tab == DetailPanelTab::Clip;
                let (bg, text_color, border_color) = if active {
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
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
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
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
                    // Check for note clip selection on this MIDI track
                    let has_note_clip = is_midi
                        && (self.state.arrangement.selected_clips.iter().any(|s| {
                            matches!(s, ArrangementSelection::NoteClip { track_id: tid, .. } if *tid == track_id)
                        }) || self
                            .state
                            .arrangement
                            .selected_note_clip
                            .is_some_and(|(tid, _)| tid == track_id));

                    if has_note_clip {
                        self.view_piano_roll_panel(track_id, track_color)
                    } else {
                        // Find a single selected audio clip on this track
                        let audio_sel =
                            self.state
                                .arrangement
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
                            if let Some(clip) = track.clips.iter().find(|c| c.id == sel_cid) {
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
                .color(th::TEXT_DIM);
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        // Ableton-style panel heights: the Devices tab is a FIXED
        // strip that device cards are designed to fit exactly (the
        // arrangement flexes instead); the Clip tab keeps flexible
        // height for the piano roll. Window-fraction heights clipped
        // cards or demanded ugly vertical scrollbars.
        let panel_height = if self.state.view.detail_panel_tab == DetailPanelTab::Devices {
            Length::Fixed(th::DEVICE_BODY_H + 96.0)
        } else {
            Length::FillPortion(2)
        };
        container(detail_content)
            .width(Length::Fill)
            .height(panel_height)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_clip_placeholder(&self) -> Element<'_, Message> {
        let label = text("Select a clip to view details")
            .size(14)
            .color(th::TEXT_DIM);
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

        let playhead_beats = self.state.position_beats();

        // Extract clip data as owned values (avoids lifetime conflicts with widget construction)
        let clip_data: Option<(String, f64, f64, bool, TrackId, ClipId)> =
            if let Some((tid, cid)) = self.state.arrangement.selected_note_clip {
                if tid == track_id {
                    self.state
                        .arrangement
                        .tracks
                        .iter()
                        .find(|t| t.id == track_id)
                        .and_then(|t| t.note_clips.iter().find(|c| c.id == cid))
                        .map(|c| {
                            (
                                c.name.clone(),
                                c.position_beats,
                                c.duration_beats,
                                c.loop_enabled,
                                tid,
                                cid,
                            )
                        })
                } else {
                    None
                }
            } else {
                None
            };

        let piano_widget = if let Some(ref cd) = clip_data {
            if let Some(track) = self.state.find_track(track_id) {
                if let Some(clip) = track.note_clips.iter().find(|c| c.id == cd.5) {
                    let clip_relative_playhead = playhead_beats - clip.position_beats;
                    PianoRollWidget::from_clip(
                        track_id,
                        clip,
                        clip_relative_playhead,
                        clip.duration_beats,
                        track_color,
                        self.state.view.snap_grid,
                        self.state.piano_roll.scroll_y,
                        self.state.piano_roll.edit_mode,
                    )
                } else {
                    PianoRollWidget::empty(track_id, playhead_beats, track_color)
                }
            } else {
                PianoRollWidget::empty(track_id, playhead_beats, track_color)
            }
        } else {
            PianoRollWidget::empty(track_id, playhead_beats, track_color)
        };

        let piano_canvas: Element<'_, Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // ── Clip properties bar (shown when a clip is selected) ──
        let mut content_col = column![].spacing(2).padding(4);

        if let Some((ref clip_name_str, clip_pos, clip_dur, clip_loop, tid, cid)) = clip_data {
            let clip_name = text(clip_name_str.clone()).size(11).color(th::TEXT);
            let pos_label = text(format!("Pos: {clip_pos:.1}"))
                .size(10)
                .color(th::TEXT_DIM);
            let dur_label = text(format!("Dur: {clip_dur:.1}"))
                .size(10)
                .color(th::TEXT_DIM);

            // Loop toggle
            let loop_icon_color = if clip_loop { th::ACCENT } else { th::TEXT_DIM };
            let loop_btn = button(icons::icon(icons::REPEAT).size(10).color(loop_icon_color))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleNoteClipLoop(
                    tid, cid,
                )))
                .padding([2, 4])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: if clip_loop {
                        Some(th::ACCENT_DIM.into())
                    } else {
                        Some(th::BG_ELEVATED.into())
                    },
                    text_color: loop_icon_color,
                    border: iced::Border {
                        color: if clip_loop {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

            // Clip operation buttons
            let op_btn_style = |_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::TEXT_DIM,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            };

            let dup_btn = button(
                row![
                    icons::icon(icons::COPY).size(10).color(th::TEXT_DIM),
                    text("Dup").size(10).color(th::TEXT_DIM)
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::duplicate_note_clip(tid, cid))
            .padding([2, 6])
            .style(op_btn_style);

            let double_btn = button(text("2x").size(10).color(th::TEXT_DIM))
                .on_press(Message::PianoRoll(PianoRollMsg::DoubleNoteClip(tid, cid)))
                .padding([2, 6])
                .style(op_btn_style);

            let halve_btn = button(text("\u{00BD}x").size(10).color(th::TEXT_DIM))
                .on_press(Message::PianoRoll(PianoRollMsg::HalveNoteClip(tid, cid)))
                .padding([2, 6])
                .style(op_btn_style);

            let crop_btn = button(
                row![
                    icons::icon(icons::SCISSORS).size(10).color(th::TEXT_DIM),
                    text("Crop").size(10).color(th::TEXT_DIM)
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::PianoRoll(PianoRollMsg::CropNoteClip(tid, cid)))
            .padding([2, 6])
            .style(op_btn_style);

            let props_row = row![
                clip_name, pos_label, dur_label, loop_btn, dup_btn, double_btn, halve_btn,
                crop_btn,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);

            content_col = content_col.push(props_row);
        }

        // ── Header row: label, edit mode toggle, snap grid ──
        let label = text("Piano Roll").size(11).color(th::TEXT_DIM);

        // Edit mode toggle: Select / Draw
        let select_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Select;
        let draw_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Draw;

        let select_btn = {
            let (bg, tc) = if select_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            button(icons::icon(icons::MOUSE_POINTER).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if select_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let draw_btn = {
            let (bg, tc) = if draw_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            button(icons::icon(icons::PENCIL).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if draw_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let mode_row = row![select_btn, draw_btn].spacing(1);

        // Snap grid selector
        use crate::state::SnapGrid;
        let mut snap_row = row![].spacing(2);
        for &grid in SnapGrid::all() {
            let is_active = self.state.view.snap_grid == grid;
            let (bg, text_color) = if is_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            let btn = button(text(grid.label()).size(10).color(text_color))
                .on_press(Message::View(ViewMsg::SetSnapGrid(grid)))
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color,
                    border: iced::Border {
                        color: if is_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            snap_row = snap_row.push(btn);
        }
        let snap_label = text("Snap:").size(10).color(th::TEXT_DIM);
        let header_row = row![label, mode_row, horizontal_space(), snap_label, snap_row]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        content_col = content_col.push(header_row).push(piano_canvas);

        container(content_col)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
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
        let playhead_samples = self.state.transport.position_samples;
        let playhead_normalized = if clip.duration > 0
            && playhead_samples >= clip.position
            && playhead_samples < clip.position + clip.duration
        {
            (playhead_samples - clip.position) as f64 / clip.duration as f64
        } else {
            -1.0
        };

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

        let label = text("Waveform").size(11).color(th::TEXT_DIM);
        let clip_info = text(format!(
            "{}: {:.1}s",
            clip.name,
            clip.duration as f64 / self.state.transport.sample_rate as f64
        ))
        .size(10)
        .color(th::TEXT_MUTED);

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
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
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
        let label = text("Warp").size(11).color(th::TEXT_DIM);

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
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => Some(th::BG_ELEVATED.into()),
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        };

        let detect_btn = button(text("Detect").size(11).color(th::TEXT))
            .on_press(Message::DetectClipBpm { track_id, clip_id })
            .padding([4, 10])
            .style(button_style);

        let warp_btn = button(
            text(format!("Warp → {:.0} BPM", self.state.transport.bpm))
                .size(11)
                .color(th::TEXT),
        )
        .on_press(Message::WarpClipToProject { track_id, clip_id })
        .padding([4, 10])
        .style(button_style);

        let mut row_widgets = row![label, bpm_input, detect_btn, warp_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        if clip.warped {
            let clear_btn = button(text("Clear warp").size(11).color(th::TEXT_DIM))
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
                            .color(th::METER_YELLOW),
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
                    .color(th::METER_YELLOW),
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
        let label = text("Quantize").size(11).color(th::TEXT_DIM);
        let grid_btn = |grid: crate::state::SnapGrid| -> Element<'_, Message> {
            button(text(grid.label()).size(11).color(th::TEXT))
                .on_press(Message::QuantizeAudioClipAt {
                    track_id,
                    clip_id,
                    grid,
                })
                .padding([4, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => Some(th::BG_ELEVATED.into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT,
                        border: iced::Border {
                            color: th::BORDER,
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
            grid_btn(crate::state::SnapGrid::Quarter),
            grid_btn(crate::state::SnapGrid::Eighth),
            grid_btn(crate::state::SnapGrid::Sixteenth),
            grid_btn(crate::state::SnapGrid::ThirtySecond),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }
}
