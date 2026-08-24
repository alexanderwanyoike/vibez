//! Audio Clip Inspector panel and waveform controls.

use iced::widget::{button, canvas, column, container, horizontal_space, row, text, text_input};
use iced::{Color, Element, Length, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::icons;
use crate::message::Message;
use crate::state::{AudioClipInspectorField, AudioClipRotaryField, UiClip};
use crate::theme as th;
use crate::widgets::audio_clip_detail::AudioClipDetailWidget;
use crate::widgets::effect_knob::EffectKnobWidget;
use crate::widgets::on_blur::on_blur;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::{ClipGainDb, ClipTranspose};

use super::views_detail::resolved_detail_playhead_samples;
use super::*;

fn audio_clip_value_input_style(
    _theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let focused = matches!(status, iced::widget::text_input::Status::Focused);
    iced::widget::text_input::Style {
        background: th::bg_surface().into(),
        border: iced::Border {
            color: if focused {
                th::accent()
            } else {
                th::border_light()
            },
            width: 1.0,
            radius: 2.0.into(),
        },
        icon: th::text_dim(),
        placeholder: th::text_dim(),
        value: th::text(),
        selection: th::accent_dim(),
    }
}

impl App {
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
            sample_rate: clip.audio.sample_rate,
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

        let label = text("WAVEFORM").size(10).color(th::text_dim());
        let clip_info = text(format!(
            "{}: {:.1}s",
            clip.name,
            clip.duration as f64 / f64::from(clip.audio.sample_rate.max(1))
        ))
        .size(10)
        .color(th::text_muted());

        let header_row = row![label, horizontal_space(), clip_info]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let sample_rate = f64::from(clip.audio.sample_rate.max(1));
        let source_start = clip.source_offset as f64 / sample_rate;
        let source_end_frames = clip
            .source_offset
            .saturating_add(clip.duration)
            .min(clip.audio.num_frames() as u64);
        let source_end = source_end_frames as f64 / sample_rate;
        let loop_start = clip.loop_start as f64 / sample_rate;
        let loop_end = clip.loop_end as f64 / sample_rate;

        let parameter = |label_text: &'static str,
                         rotary_field: AudioClipRotaryField,
                         current_value: f32,
                         committed: String,
                         unit: &'static str,
                         min: f32,
                         max: f32| {
            let inspector_field = rotary_field.inspector_field();
            let knob_value = self
                .state
                .active_timeline_editor()
                .audio_clip_inspector_edits
                .get(&(clip.id, inspector_field))
                .and_then(|text| text.parse::<f32>().ok())
                .unwrap_or(current_value);
            let knob = canvas(EffectKnobWidget::for_audio_clip(
                track_id,
                clip.id,
                rotary_field,
                knob_value,
                min,
                max,
                0.0,
                track_color,
            ))
            .width(Length::Fixed(34.0))
            .height(Length::Fixed(34.0));
            container(
                row![
                    column![
                        text(label_text).size(8).color(th::text_muted()),
                        row![
                            self.view_audio_clip_field_input(
                                track_id,
                                clip.id,
                                inspector_field,
                                committed,
                                52.0,
                            ),
                            text(unit).size(8).color(th::text_dim()),
                        ]
                        .spacing(4)
                        .align_y(iced::Alignment::Center),
                    ]
                    .spacing(3),
                    knob,
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding([4, 6])
            .width(Length::Fixed(116.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::divider(),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            })
        };
        let gain_and_pitch = column![
            parameter(
                "GAIN",
                AudioClipRotaryField::Gain,
                clip.gain_db.db(),
                format!("{:.1}", clip.gain_db.db()),
                "dB",
                ClipGainDb::MIN,
                ClipGainDb::MAX,
            ),
            parameter(
                "PITCH",
                AudioClipRotaryField::Transpose,
                f32::from(clip.transpose.semitones()),
                clip.transpose.semitones().to_string(),
                "st",
                f32::from(ClipTranspose::MIN),
                f32::from(ClipTranspose::MAX),
            ),
        ]
        .spacing(6);

        let range_field = |label_text: &'static str,
                           inspector_field: AudioClipInspectorField,
                           committed: String| {
            column![
                text(label_text).size(8).color(th::text_dim()),
                self.view_audio_clip_field_input(
                    track_id,
                    clip.id,
                    inspector_field,
                    committed,
                    92.0,
                ),
            ]
            .spacing(3)
        };
        let range_row = |start_field: AudioClipInspectorField,
                         start_value: String,
                         end_field: AudioClipInspectorField,
                         end_value: String| {
            row![
                range_field("START", start_field, start_value,),
                range_field("END", end_field, end_value,),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
        };

        let loop_enabled = clip.loop_enabled;
        let loop_color = if loop_enabled {
            th::accent()
        } else {
            th::text_dim()
        };
        let loop_button = button(
            text(if loop_enabled { "LOOP ON" } else { "LOOP OFF" })
                .size(9)
                .color(loop_color),
        )
        .on_press(Message::Arrangement(ArrangementMsg::ToggleClipLoop(
            track_id, clip.id,
        )))
        .padding([3, 7])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(if loop_enabled {
                th::accent_dim().into()
            } else {
                th::bg_elevated().into()
            }),
            text_color: loop_color,
            border: iced::Border {
                color: if loop_enabled {
                    th::accent()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

        let divider = || {
            container(horizontal_space())
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::divider().into()),
                    ..Default::default()
                })
        };
        let clip_identity = row![
            icons::icon(icons::AUDIO_WAVEFORM)
                .size(13)
                .color(track_color),
            column![
                text("AUDIO CLIP").size(8).color(th::text_muted()),
                text(clip.name.clone())
                    .size(11)
                    .color(th::text())
                    .width(Length::Fill),
            ]
            .spacing(1)
            .width(Length::Fill),
        ]
        .spacing(7)
        .align_y(iced::Alignment::Center);
        let source_bounds = column![
            text("SOURCE").size(8).color(th::text_muted()),
            range_row(
                AudioClipInspectorField::SourceStart,
                format!("{source_start:.3}"),
                AudioClipInspectorField::SourceEnd,
                format!("{source_end:.3}"),
            ),
        ]
        .spacing(3);
        let loop_bounds = column![
            row![loop_button].align_y(iced::Alignment::Center),
            range_row(
                AudioClipInspectorField::LoopStart,
                format!("{loop_start:.3}"),
                AudioClipInspectorField::LoopEnd,
                format!("{loop_end:.3}"),
            ),
        ]
        .spacing(3);

        let timing_controls = column![
            self.view_audio_warp_row(track_id, clip),
            divider(),
            source_bounds,
            loop_bounds,
        ]
        .spacing(6)
        .width(Length::Fill);
        let inspector_body = row![timing_controls, gain_and_pitch]
            .spacing(8)
            .align_y(iced::Alignment::Start);
        let inspector = container(
            column![clip_identity, inspector_body]
                .spacing(6)
                .padding([7, 8]),
        )
        .width(Length::Fixed(388.0))
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_dark().into()),
            border: iced::Border {
                color: th::divider(),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

        let quantize_row = self.view_audio_quantize_row(track_id, clip.id);
        let waveform = column![header_row, quantize_row, waveform_canvas]
            .spacing(6)
            .padding(4)
            .width(Length::Fill)
            .height(Length::Fill);
        let content = row![inspector, waveform].spacing(6).padding(4);

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
        let default_text = clip
            .original_bpm
            .map(|bpm| format!("{:.1}", bpm))
            .unwrap_or_default();
        let text_value = self
            .state
            .active_timeline_editor()
            .audio_clip_inspector_edits
            .get(&(clip_id, AudioClipInspectorField::SourceBpm))
            .cloned()
            .unwrap_or(default_text);

        let bpm_pending = self
            .state
            .active_timeline_editor()
            .audio_clip_inspector_edits
            .contains_key(&(clip_id, AudioClipInspectorField::SourceBpm));
        let bpm_input = text_input("BPM", &text_value)
            .on_input(move |t| {
                Message::Arrangement(ArrangementMsg::AudioClipInspectorInputChanged {
                    clip_id,
                    field: AudioClipInspectorField::SourceBpm,
                    text: t,
                })
            })
            .on_submit(Message::Arrangement(
                ArrangementMsg::SubmitAudioClipInspectorField {
                    track_id,
                    clip_id,
                    field: AudioClipInspectorField::SourceBpm,
                },
            ))
            .size(10)
            .padding([2, 5])
            .width(Length::Fixed(56.0))
            .style(audio_clip_value_input_style);
        let bpm_input = on_blur(
            bpm_input,
            bpm_pending,
            Message::Arrangement(ArrangementMsg::DiscardAudioClipInspectorEdit {
                clip_id,
                field: AudioClipInspectorField::SourceBpm,
            }),
        );

        let utility_button_style = |_theme: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => Some(th::bg_surface().into()),
            };
            button::Style {
                background: bg,
                text_color: th::text_dim(),
                border: iced::Border {
                    color: th::divider(),
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        };

        let detect_btn = button(text("DETECT").size(8).color(th::text_dim()))
            .on_press(Message::DetectClipBpm {
                location: self.active_timeline_location(),
                track_id,
                clip_id,
            })
            .padding([3, 6])
            .style(utility_button_style);

        let warped = clip.warped;
        let warp_message = if warped {
            Message::Arrangement(ArrangementMsg::ClearClipWarp { track_id, clip_id })
        } else {
            Message::WarpClipToProject {
                location: self.active_timeline_location(),
                track_id,
                clip_id,
            }
        };
        let warp_button = button(
            text(if warped { "WARP ON" } else { "WARP OFF" })
                .size(10)
                .color(if warped { th::accent() } else { th::text_dim() }),
        )
        .on_press(warp_message)
        .padding([5, 10])
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if warped {
                        th::accent_dim()
                    } else if hovered {
                        th::bg_hover()
                    } else {
                        th::bg_surface()
                    }
                    .into(),
                ),
                text_color: if warped { th::accent() } else { th::text_dim() },
                border: iced::Border {
                    color: if warped { th::accent() } else { th::divider() },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        });
        let warp_is_stale = clip
            .warped_to_bpm
            .is_some_and(|bpm| (bpm - self.state.transport.bpm).abs() > 0.01);
        let warp_target = text(if warped {
            format!("TO {:.0} BPM", clip.warped_to_bpm.unwrap_or_default())
        } else {
            "SOURCE TIMING".into()
        })
        .size(8)
        .color(if warp_is_stale {
            th::meter_yellow()
        } else if warped {
            th::accent()
        } else {
            th::text_muted()
        });

        column![
            row![
                warp_button,
                horizontal_space(),
                warp_target,
                if warp_is_stale {
                    text("OUT OF TEMPO").size(8).color(th::meter_yellow())
                } else {
                    text("").size(8)
                },
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
            row![
                text("SOURCE BPM").size(8).color(th::text_muted()),
                horizontal_space(),
                bpm_input,
                detect_btn,
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(5)
        .into()
    }

    fn view_audio_clip_field_input(
        &self,
        track_id: TrackId,
        clip_id: ClipId,
        field: AudioClipInspectorField,
        committed: String,
        width: f32,
    ) -> Element<'_, Message> {
        let value = self
            .state
            .active_timeline_editor()
            .audio_clip_inspector_edits
            .get(&(clip_id, field))
            .cloned()
            .unwrap_or(committed);
        let pending = self
            .state
            .active_timeline_editor()
            .audio_clip_inspector_edits
            .contains_key(&(clip_id, field));
        let input = text_input("", &value)
            .on_input(move |text| {
                Message::Arrangement(ArrangementMsg::AudioClipInspectorInputChanged {
                    clip_id,
                    field,
                    text,
                })
            })
            .on_submit(Message::Arrangement(
                ArrangementMsg::SubmitAudioClipInspectorField {
                    track_id,
                    clip_id,
                    field,
                },
            ))
            .size(11)
            .padding([4, 6])
            .width(Length::Fixed(width))
            .style(audio_clip_value_input_style);
        on_blur(
            input,
            pending,
            Message::Arrangement(ArrangementMsg::DiscardAudioClipInspectorEdit { clip_id, field }),
        )
        .into()
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
