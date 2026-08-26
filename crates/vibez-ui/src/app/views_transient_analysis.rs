//! Transient analysis modal for the Audio Clip waveform.

use iced::widget::{button, center, column, container, horizontal_space, mouse_area, row, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;

use super::*;

fn detail_choice(
    label: &'static str,
    purpose: &'static str,
    detail: vibez_core::onset::TransientDetectionDetail,
    selected: bool,
) -> Element<'static, Message> {
    button(
        column![
            text(label)
                .size(13)
                .color(if selected { th::accent() } else { th::text() }),
            text(purpose).size(10).color(th::text_dim()),
        ]
        .spacing(4),
    )
    .on_press(Message::SetTransientAnalysisDetail(detail))
    .padding([10, 11])
    .width(Length::Fill)
    .style(move |_theme: &Theme, status| button::Style {
        background: Some(
            if selected {
                th::accent_dim()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text(),
        border: iced::Border {
            color: if selected { th::accent() } else { th::border() },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

impl App {
    pub(super) fn view_transient_analysis_overlay(&self) -> Element<'_, Message> {
        use vibez_core::onset::TransientDetectionDetail::{Balanced, Fewer, More};

        let dialog = self
            .state
            .view
            .transient_analysis_dialog
            .expect("Transient analysis overlay requires a pending request");
        let clip = self
            .timeline_content_at(dialog.location, dialog.track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == dialog.clip_id));
        let clip_name = clip.map_or("Audio Clip", |clip| clip.name.as_str());
        let marker_count = |kind| {
            clip.map_or(0, |clip| {
                clip.transient_markers
                    .as_slice()
                    .iter()
                    .filter(|marker| marker.kind() == kind)
                    .count()
            })
        };
        let detected_count = marker_count(vibez_core::transient::TransientMarkerKind::Suggested);
        let manual_count = marker_count(vibez_core::transient::TransientMarkerKind::Authored);
        let guidance = match dialog.detail {
            Fewer => {
                "Keeps the strongest attacks. Useful when a busy loop creates too many slices."
            }
            Balanced => "A practical starting point for most drum and percussion loops.",
            More => "Includes quieter attacks and hits that sit close together.",
        };
        let marker_summary = if manual_count == 0 {
            format!("{detected_count} detected markers")
        } else {
            format!("{detected_count} detected · {manual_count} manual")
        };
        let cancel = button(text("Cancel").size(12).color(th::text()))
            .on_press(Message::CancelTransientAnalysis)
            .padding([7, 14]);
        let analyse = button(text("Analyse").size(12).color(th::bg_dark()))
            .on_press(Message::ConfirmTransientAnalysis)
            .padding([7, 18])
            .style(|_theme: &Theme, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        th::accent_dim()
                    } else {
                        th::accent()
                    }
                    .into(),
                ),
                text_color: th::bg_dark(),
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let card = container(
            column![
                row![
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(17)
                        .color(th::accent()),
                    text("Transient analysis").size(18).color(th::text()),
                    horizontal_space(),
                    text(marker_summary).size(10).color(th::text_muted()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                text(clip_name).size(11).color(th::text_dim()),
                text("Choose how closely markers should follow the waveform.")
                    .size(11)
                    .color(th::text_dim()),
                row![
                    detail_choice("Fewer", "Strong hits", Fewer, dialog.detail == Fewer),
                    detail_choice(
                        "Balanced",
                        "Most loops",
                        Balanced,
                        dialog.detail == Balanced,
                    ),
                    detail_choice("More", "Fine detail", More, dialog.detail == More),
                ]
                .spacing(8),
                container(text(guidance).size(11).color(th::text_dim()))
                    .width(Length::Fill)
                    .padding([9, 11])
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::bg_elevated().into()),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }),
                text("Manual markers stay in place when analysis runs.")
                    .size(10)
                    .color(th::text_muted()),
                row![horizontal_space(), cancel, analyse]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
            ]
            .spacing(12),
        )
        .width(Length::Fixed(500.0))
        .padding(18)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        mouse_area(
            center(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelTransientAnalysis)
        .into()
    }
}
