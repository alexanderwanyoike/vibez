//! Compact Section Record and Capture strip shared by every Perform mode.

use iced::widget::{button, container, horizontal_space, pick_list, row, text};
use iced::{Element, Length, Theme};

use crate::domains::perform::{
    CaptureMsg, CapturePhase, PerformMsg, SectionRecordCountIn, SectionRecordMode,
    SectionRecordMsg, SectionRecordPhase, SectionRecordQuantization,
};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

use super::*;

impl App {
    pub(super) fn view_section_record_bar(&self) -> Element<'_, Message> {
        let record = &self.state.perform.section_record;
        let capture = &self.state.perform.capture;
        let active = record.is_active();
        let recording = record.phase == SectionRecordPhase::Recording;
        let capture_active = capture.is_active();
        let capturing = capture.phase == CapturePhase::Recording;
        let target = record
            .target()
            .and_then(|(section_id, track_id)| {
                let section = self.state.perform.sections.by_id(section_id)?;
                let track = self.state.find_track(track_id)?;
                Some(format!("REC TARGET · {} · {}", section.name, track.name))
            })
            .unwrap_or_else(|| "PLAYING SECTION · INSTRUMENT TARGET".into());
        let phase = match record.phase {
            SectionRecordPhase::Idle => "READY".into(),
            SectionRecordPhase::Preparing => "PREPARING".into(),
            SectionRecordPhase::Armed => record
                .pending_boundary_samples
                .and_then(|boundary| {
                    (self.state.perform.playing_section.is_none())
                        .then(|| {
                            count_in_beats_remaining(
                                boundary,
                                self.state.transport.position_samples,
                                self.state.transport.bpm,
                                self.state.transport.sample_rate,
                            )
                        })
                        .flatten()
                })
                .map(|beats| format!("COUNT-IN · {beats}"))
                .unwrap_or_else(|| "ARMED · NEXT BAR".into()),
            SectionRecordPhase::Recording => "RECORDING".into(),
            SectionRecordPhase::Stopping => "STOPPING".into(),
        };
        let capture_phase = match capture.phase {
            CapturePhase::Idle => "ARRANGE · READY",
            CapturePhase::Starting => "ARRANGE · STARTING",
            CapturePhase::Recording => "ARRANGE · CAPTURING",
            CapturePhase::Stopping => "ARRANGE · STOPPING",
        };

        let record_button = button(record_button_content(active))
            .on_press(Message::Perform(PerformMsg::SectionRecord(
                SectionRecordMsg::Toggle,
            )))
            .padding([7, 12])
            .style(move |_theme: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(
                        if recording {
                            th::danger()
                        } else if active || hovered {
                            th::accent()
                        } else {
                            th::perform_inset()
                        }
                        .into(),
                    ),
                    text_color: if active { th::bg_dark() } else { th::danger() },
                    border: iced::Border {
                        color: if active {
                            th::danger()
                        } else {
                            th::border_light()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            });

        let capture_button = button(capture_button_content(capture_active))
            .on_press(Message::Perform(PerformMsg::Capture(CaptureMsg::Toggle)))
            .padding([7, 12])
            .style(move |_theme: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let emphasized = capture_active || hovered;
                button::Style {
                    background: Some(
                        if capturing {
                            th::danger()
                        } else if capture_active {
                            th::accent()
                        } else if hovered {
                            th::blend(th::perform_inset(), th::accent(), 0.14)
                        } else {
                            th::perform_inset()
                        }
                        .into(),
                    ),
                    text_color: if emphasized {
                        th::bg_dark()
                    } else {
                        th::accent()
                    },
                    border: iced::Border {
                        color: if capturing {
                            th::danger()
                        } else {
                            th::accent_dim()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            });

        let count_in = record_pick_list(
            &SectionRecordCountIn::ALL,
            record.count_in,
            SectionRecordMsg::SetCountIn,
            126.0,
        );
        let mode = record_pick_list(
            &SectionRecordMode::ALL,
            record.mode,
            SectionRecordMsg::SetMode,
            94.0,
        );
        let quantization = record_pick_list(
            &SectionRecordQuantization::ALL,
            record.quantization,
            SectionRecordMsg::SetQuantization,
            96.0,
        );
        let state_color = if recording {
            th::danger()
        } else if active {
            th::accent()
        } else {
            th::text_dim()
        };

        container(
            row![
                record_button,
                count_in,
                mode,
                quantization,
                capture_button,
                container(horizontal_space()).width(Length::Fill),
                row![
                    text(target)
                        .font(PERFORM_TECH)
                        .size(9)
                        .color(th::text_dim()),
                    text(phase).font(PERFORM_LABEL).size(9).color(state_color),
                    text(capture_phase)
                        .font(PERFORM_LABEL)
                        .size(9)
                        .color(if capturing {
                            th::danger()
                        } else {
                            th::accent()
                        }),
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(7)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(42.0))
        .padding([5, 12])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::divider(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
    }
}

fn count_in_beats_remaining(
    boundary: u64,
    position: u64,
    bpm: f64,
    sample_rate: u32,
) -> Option<u64> {
    if boundary <= position || !bpm.is_finite() || bpm <= 0.0 || sample_rate == 0 {
        return None;
    }
    let beat_samples = (f64::from(sample_rate) * 60.0 / bpm).round().max(1.0) as u64;
    let remaining = boundary - position;
    Some(remaining.saturating_add(beat_samples - 1) / beat_samples)
}

fn record_button_content(active: bool) -> Element<'static, Message> {
    let color = if active { th::bg_dark() } else { th::danger() };
    row![
        icons::icon(if active {
            icons::STOP
        } else {
            icons::CIRCLE_DOT
        })
        .size(11)
        .color(color),
        text(if active {
            "STOP SECTION"
        } else {
            "SECTION REC"
        })
        .font(PERFORM_TECH_STRONG)
        .size(9)
        .color(color),
        shortcut_keycap("F4", color, active),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn capture_button_content(active: bool) -> Element<'static, Message> {
    let color = if active { th::bg_dark() } else { th::accent() };
    row![
        icons::icon(if active {
            icons::STOP
        } else {
            icons::LAYOUT_LIST
        })
        .size(11)
        .color(color),
        text(if active {
            "STOP CAPTURE"
        } else {
            "CAPTURE → ARRANGE"
        })
        .font(PERFORM_TECH_STRONG)
        .size(9)
        .color(color),
        shortcut_keycap("F5", color, active),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn shortcut_keycap(
    label: &'static str,
    color: iced::Color,
    active: bool,
) -> Element<'static, Message> {
    container(text(label).font(PERFORM_TECH_STRONG).size(8).color(color))
        .padding([1, 4])
        .style(move |_theme: &Theme| container::Style {
            background: (!active).then(|| th::blend(th::bg_dark(), color, 0.08).into()),
            border: iced::Border {
                color: th::blend(th::border_light(), color, 0.3),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn record_pick_list<'a, T: Copy + Eq + std::fmt::Display + 'static>(
    options: &'static [T],
    selected: T,
    message: impl Fn(T) -> SectionRecordMsg + 'a,
    width: f32,
) -> Element<'a, Message> {
    pick_list(options, Some(selected), move |value| {
        Message::Perform(PerformMsg::SectionRecord(message(value)))
    })
    .width(Length::Fixed(width))
    .padding([5, 7])
    .text_size(9)
    .into()
}

#[cfg(test)]
mod tests {
    use super::count_in_beats_remaining;

    #[test]
    fn count_in_label_counts_whole_beats_down_to_one() {
        assert_eq!(count_in_beats_remaining(88_200, 0, 120.0, 44_100), Some(4));
        assert_eq!(
            count_in_beats_remaining(88_200, 22_049, 120.0, 44_100),
            Some(4)
        );
        assert_eq!(
            count_in_beats_remaining(88_200, 22_050, 120.0, 44_100),
            Some(3)
        );
        assert_eq!(
            count_in_beats_remaining(88_200, 88_199, 120.0, 44_100),
            Some(1)
        );
        assert_eq!(
            count_in_beats_remaining(88_200, 88_200, 120.0, 44_100),
            None
        );
    }
}
