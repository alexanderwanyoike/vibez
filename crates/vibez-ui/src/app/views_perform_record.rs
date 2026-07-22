//! Compact Section Record strip shared by every Perform mode.

use iced::widget::{button, container, horizontal_space, pick_list, row, text};
use iced::{Element, Length, Theme};

use crate::domains::perform::{
    PerformMsg, SectionRecordCountIn, SectionRecordMode, SectionRecordMsg, SectionRecordPhase,
    SectionRecordQuantization,
};
use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH, PERFORM_TECH_STRONG};

use super::*;

impl App {
    pub(super) fn view_section_record_bar(&self) -> Element<'_, Message> {
        let record = &self.state.perform.section_record;
        let active = record.is_active();
        let recording = record.phase == SectionRecordPhase::Recording;
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
                .map(|sample| format!("PENDING · {sample} smp"))
                .unwrap_or_else(|| "ARMED".into()),
            SectionRecordPhase::Recording => "RECORDING".into(),
            SectionRecordPhase::Stopping => "STOPPING".into(),
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
                container(horizontal_space()).width(Length::Fill),
                row![
                    text(target)
                        .font(PERFORM_TECH)
                        .size(9)
                        .color(th::text_dim()),
                    text(phase).font(PERFORM_LABEL).size(9).color(state_color),
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

fn record_button_content(active: bool) -> Element<'static, Message> {
    let color = if active { th::bg_dark() } else { th::danger() };
    let shortcut = container(text("F4").font(PERFORM_TECH_STRONG).size(8).color(color))
        .padding([1, 4])
        .style(move |_theme: &Theme| container::Style {
            background: (!active).then(|| th::blend(th::bg_dark(), color, 0.08).into()),
            border: iced::Border {
                color: th::blend(th::border_light(), color, 0.3),
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });
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
        shortcut,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
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
