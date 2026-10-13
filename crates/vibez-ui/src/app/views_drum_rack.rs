use super::*;
use crate::message::DrumPadParam;
use iced::widget::{button, canvas, column, container, horizontal_space, mouse_area, row, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrumPadVisualState {
    Idle,
    Selected,
    Firing,
}

fn drum_pad_visual_state(selected: bool, firing: bool) -> DrumPadVisualState {
    if firing {
        DrumPadVisualState::Firing
    } else if selected {
        DrumPadVisualState::Selected
    } else {
        DrumPadVisualState::Idle
    }
}

impl App {
    pub(super) fn view_drum_rack_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::{param_column, EffectKnobWidget};
        let selected_pad = track
            .selected_drum_pad
            .min(track.drum_rack_pads.len().saturating_sub(1));
        let bank_size = vibez_core::track::DRUM_RACK_BANK_SIZE;
        let bank_count = vibez_core::track::DRUM_RACK_BANK_COUNT;
        let selected_bank = (selected_pad / bank_size).min(bank_count.saturating_sub(1));
        let bank_start = selected_bank * bank_size;
        let title = Self::device_title_bar(Self::device_title_row(
            "Drum Rack",
            track_color,
            Some(Message::remove_track_instrument(track_id)),
        ));

        let activity_now = std::time::Instant::now();
        let earlier_bank_firing = (0..selected_bank).any(|bank| {
            self.state
                .view
                .drum_pad_bank_is_flashing(track_id, bank, activity_now)
        });
        let later_bank_firing = ((selected_bank + 1)..bank_count).any(|bank| {
            self.state
                .view
                .drum_pad_bank_is_flashing(track_id, bank, activity_now)
        });
        let mut grid = column![].spacing(4);
        for row_index in 0..4 {
            let mut pad_row = row![].spacing(4);
            for col_index in 0..4 {
                let pad_index = bank_start + row_index * 4 + col_index;
                let pad = &track.drum_rack_pads[pad_index];
                let pad_pitch = vibez_core::track::drum_rack_pad_pitch(pad_index)
                    .expect("Drum Rack UI renders only valid pad slots");
                let visual_state = drum_pad_visual_state(
                    selected_pad == pad_index,
                    self.state
                        .view
                        .drum_pad_is_flashing(track_id, pad_pitch, activity_now),
                );
                let firing = visual_state == DrumPadVisualState::Firing;
                let selected = visual_state == DrumPadVisualState::Selected;
                let drop_target = matches!(
                    self.state.browser.drag_target,
                    Some(crate::state::BrowserDropTarget::DrumRackPad {
                        track_id: target_track,
                        pad_index: target_pad,
                    }) if target_track == track_id && target_pad == pad_index
                );
                // Hard-truncated single line: wrapping names change
                // the tile height and blow the card's height budget
                // (clipped knobs, dogfood screenshot 2026-07-06).
                let label = pad
                    .name
                    .as_deref()
                    .map(|name| {
                        let short: String = name.chars().take(6).collect();
                        if name.chars().count() > 6 {
                            format!("{short}..")
                        } else {
                            short
                        }
                    })
                    .unwrap_or_else(|| format!("Pad {}", pad_index + 1));
                // Use container + mouse_area so press events reach us and
                // drag-drop works. iced Button would capture ButtonPressed
                // and hide it from mouse_area.
                let pad_note = crate::widgets::piano_roll::pitch_name(pad_pitch);
                let pad_body = container(
                    column![
                        text(format!("{:02}  {pad_note}", pad_index + 1))
                            .size(9)
                            .color(if firing {
                                th::bg_dark()
                            } else if selected {
                                th::accent()
                            } else {
                                th::text_dim()
                            }),
                        text(label).size(8).color(if firing {
                            th::bg_dark()
                        } else if selected {
                            th::accent()
                        } else {
                            th::text()
                        })
                    ]
                    .spacing(2)
                    .align_x(iced::Alignment::Center),
                )
                .padding([3, 4])
                .width(Length::Fixed(52.0))
                .height(Length::Fixed(30.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(
                        if firing {
                            th::accent()
                        } else if drop_target || selected {
                            th::accent_dim()
                        } else {
                            th::bg_dark()
                        }
                        .into(),
                    ),
                    text_color: Some(if firing { th::bg_dark() } else { th::text() }),
                    border: iced::Border {
                        color: if firing || drop_target {
                            th::accent()
                        } else if selected {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: if firing || drop_target { 2.0 } else { 1.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                });
                // on_release handler selects the pad when no drag is active,
                // otherwise routes through DropSampleOnDrumPad.
                let pad_cell: Element<'a, Message> = mouse_area(pad_body)
                    .on_enter(Message::Browser(BrowserMsg::DragHoverDrumRackPad {
                        track_id,
                        pad_index,
                    }))
                    .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
                    .on_release(Message::DropSampleOnDrumPad {
                        track_id,
                        pad_index,
                    })
                    .into();
                pad_row = pad_row.push(pad_cell);
            }
            grid = grid.push(pad_row);
        }

        let bank_button = |icon: char, bank: Option<usize>, firing: bool| {
            button(icons::icon(icon).size(9).color(if firing {
                th::accent()
            } else {
                th::text_dim()
            }))
            .on_press_maybe(bank.map(|bank| {
                Message::Devices(crate::domains::devices::DevicesMsg::SelectDrumRackBank(
                    track_id, bank,
                ))
            }))
            .padding([1, 7])
            .style(move |_theme: &Theme, status| button::Style {
                background: Some(
                    if firing {
                        th::accent_dim()
                    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        th::bg_hover()
                    } else {
                        th::bg_dark()
                    }
                    .into(),
                ),
                text_color: if firing { th::accent() } else { th::text_dim() },
                border: iced::Border {
                    color: if firing { th::accent() } else { th::border() },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            })
        };
        let bank_navigation = row![
            text(format!("BANK {} / {bank_count}", selected_bank + 1))
                .size(9)
                .color(th::text_muted()),
            horizontal_space(),
            bank_button(
                icons::CHEVRON_LEFT,
                selected_bank.checked_sub(1),
                earlier_bank_firing
            ),
            bank_button(
                icons::CHEVRON_RIGHT,
                (selected_bank + 1 < bank_count).then_some(selected_bank + 1),
                later_bank_firing
            ),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center);

        let selected_name = track.drum_rack_pads[selected_pad]
            .name
            .clone()
            .unwrap_or_else(|| "No sample loaded".to_string());
        let source_hint = track.drum_rack_pads[selected_pad]
            .source
            .as_ref()
            .map(MediaSourceRef::display_name)
            .unwrap_or_else(|| "Use the browser or Load".to_string());
        let selected_pad_state = &track.drum_rack_pads[selected_pad];

        let load_btn = button(text("Load").size(9).color(th::text()))
            .on_press(Message::LoadDrumRackPadSample(track_id, selected_pad))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => th::bg_hover(),
                    _ => th::bg_dark(),
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::text(),
                    ..Default::default()
                }
            });

        let clear_btn = button(text("Clear").size(9).color(th::text_dim()))
            .on_press(Message::clear_drum_rack_pad(track_id, selected_pad))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => Some(th::bg_dark().into()),
                };
                button::Style {
                    background: bg,
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::text_dim(),
                    ..Default::default()
                }
            });

        let footer = column![
            text(truncate_end(&selected_name, 22))
                .size(10)
                .color(th::text()),
            text(truncate_end(&source_hint, 26))
                .size(9)
                .color(th::text_dim()),
            row![load_btn, clear_btn]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(4);

        let drum_params = [
            (
                "Gain",
                format!("{:.2}", selected_pad_state.gain),
                selected_pad_state.gain,
                0.0,
                2.0,
                1.0,
                DrumPadParam::Gain,
            ),
            (
                "Pan",
                format!("{:.2}", selected_pad_state.pan),
                selected_pad_state.pan,
                -1.0,
                1.0,
                0.0,
                DrumPadParam::Pan,
            ),
            (
                "Start",
                format!("{:.0}%", selected_pad_state.start * 100.0),
                selected_pad_state.start,
                0.0,
                1.0,
                0.0,
                DrumPadParam::Start,
            ),
            (
                "End",
                format!("{:.0}%", selected_pad_state.end * 100.0),
                selected_pad_state.end,
                0.0,
                1.0,
                1.0,
                DrumPadParam::End,
            ),
            (
                "Fade In",
                format!("{:.0}ms", selected_pad_state.fade_in_ms),
                selected_pad_state.fade_in_ms,
                0.0,
                vibez_core::track::DRUM_PAD_MAX_FADE_MS,
                vibez_core::track::DRUM_PAD_DEFAULT_FADE_MS,
                DrumPadParam::FadeIn,
            ),
            (
                "Fade Out",
                format!("{:.0}ms", selected_pad_state.fade_out_ms),
                selected_pad_state.fade_out_ms,
                0.0,
                vibez_core::track::DRUM_PAD_MAX_FADE_MS,
                vibez_core::track::DRUM_PAD_DEFAULT_FADE_MS,
                DrumPadParam::FadeOut,
            ),
            (
                "Coarse",
                format!("{}st", selected_pad_state.coarse_tune),
                selected_pad_state.coarse_tune as f32,
                -24.0,
                24.0,
                0.0,
                DrumPadParam::CoarseTune,
            ),
            (
                "Fine",
                format!("{:.0}ct", selected_pad_state.fine_tune),
                selected_pad_state.fine_tune,
                -100.0,
                100.0,
                0.0,
                DrumPadParam::FineTune,
            ),
        ];

        let mut knob_row = row![].spacing(6);
        for (label_text, value_text, value, min, max, default, param) in drum_params.iter() {
            let knob = EffectKnobWidget::for_drum_pad(
                track_id,
                selected_pad,
                *param,
                *value,
                *min,
                *max,
                *default,
                track_color,
            );
            knob_row = knob_row.push(param_column(
                knob,
                label_text.to_string(),
                value_text.clone(),
            ));
        }

        let one_shot_active = selected_pad_state.one_shot;
        let one_shot_btn = button(text("One-shot").size(9).color(if one_shot_active {
            th::accent()
        } else {
            th::text_dim()
        }))
        .on_press(Message::Devices(
            crate::domains::devices::DevicesMsg::SetDrumPadOneShot {
                track_id,
                pad_index: selected_pad,
                one_shot: !one_shot_active,
            },
        ))
        .padding([2, 6])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(
                if one_shot_active {
                    th::accent_dim()
                } else {
                    th::bg_dark()
                }
                .into(),
            ),
            text_color: if one_shot_active {
                th::accent()
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: if one_shot_active {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

        let mut choke_row = row![text("Choke").size(9).color(th::text_dim())]
            .spacing(2)
            .align_y(iced::Alignment::Center);
        for (group, label) in [
            (None, "Off"),
            (Some(1), "1"),
            (Some(2), "2"),
            (Some(3), "3"),
            (Some(4), "4"),
        ] {
            let active = selected_pad_state.choke_group == group;
            let btn = button(text(label).size(9).color(if active {
                th::accent()
            } else {
                th::text_dim()
            }))
            .on_press(Message::Devices(
                crate::domains::devices::DevicesMsg::SetDrumPadChokeGroup {
                    track_id,
                    pad_index: selected_pad,
                    choke_group: group,
                },
            ))
            .padding([2, 6])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(
                    if active {
                        th::accent_dim()
                    } else {
                        th::bg_dark()
                    }
                    .into(),
                ),
                text_color: if active { th::accent() } else { th::text_dim() },
                border: iced::Border {
                    color: if active {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });
            choke_row = choke_row.push(btn);
        }

        let pad_wave: Element<'a, Message> = canvas(crate::widgets::mini_waveform::MiniWaveform {
            audio: track.drum_rack_pads[selected_pad].audio.clone(),
            color: track_color,
            region: Some((selected_pad_state.start, selected_pad_state.end)),
        })
        .width(Length::Fixed(190.0))
        .height(Length::Fixed(40.0))
        .into();
        let editor = column![
            row![footer, pad_wave]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            knob_row,
            row![one_shot_btn, choke_row]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(6);

        // Pads and the selected pad's editor sit side by side. The rack owns
        // four banks but renders one fixed 4x4 surface at a time, so adding
        // pads never changes the device card's footprint.
        let pads_view: Element<'a, Message> = column![bank_navigation, grid]
            .spacing(5)
            .width(Length::Fixed(232.0))
            .into();

        let body = row![
            Self::device_section("PADS", pads_view),
            Self::device_divider(),
            Self::device_section("PAD", editor.into()),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Start);

        // Pads + eight full-size pad controls + chrome. Keep the card wide
        // enough that adding controls never compresses either section.
        Self::device_card(
            column![title, Self::device_body(body.into())].width(Length::Fixed(780.0)),
        )
    }
}

#[cfg(test)]
mod drum_pad_visual_tests {
    use super::*;

    #[test]
    fn firing_has_precedence_over_selection() {
        assert_eq!(
            drum_pad_visual_state(true, true),
            DrumPadVisualState::Firing
        );
        assert_eq!(
            drum_pad_visual_state(true, false),
            DrumPadVisualState::Selected
        );
        assert_eq!(
            drum_pad_visual_state(false, false),
            DrumPadVisualState::Idle
        );
    }
}
