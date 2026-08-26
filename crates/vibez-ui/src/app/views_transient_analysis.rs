//! Transient analysis modal for the Audio Clip waveform.

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::transient_sensitivity_knob::TransientSensitivityKnob;

use super::*;

impl App {
    pub(super) fn view_transient_analysis_overlay(&self) -> Element<'_, Message> {
        let dialog = self
            .state
            .view
            .transient_analysis_dialog
            .as_ref()
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
        let marker_summary = if manual_count == 0 {
            format!("{detected_count} currently detected")
        } else {
            format!("{detected_count} currently detected · {manual_count} manual")
        };

        let sensitivity_knob: Element<'_, Message> =
            canvas(TransientSensitivityKnob::new(dialog.sensitivity.percent()))
                .width(Length::Fixed(68.0))
                .height(Length::Fixed(68.0))
                .into();
        let sensitivity_input = text_input("0–100", &dialog.sensitivity_input)
            .on_input(Message::TransientAnalysisSensitivityInputChanged)
            .on_submit(Message::SubmitTransientAnalysisSensitivity)
            .width(Length::Fixed(62.0))
            .padding([5, 7])
            .size(13);
        let sensitivity_control = container(
            row![
                column![
                    sensitivity_knob,
                    text("Drag or scroll").size(9).color(th::text_muted())
                ]
                .align_x(iced::Alignment::Center)
                .spacing(4),
                column![
                    text("SENSITIVITY").size(9).color(th::text_muted()),
                    row![sensitivity_input, text("%").size(13).color(th::text_dim()),]
                        .spacing(5)
                        .align_y(iced::Alignment::Center),
                    text("Higher values keep quieter attacks and create more markers.")
                        .size(11)
                        .color(th::text_dim()),
                    row![
                        text("Strong attacks").size(9).color(th::text_muted()),
                        horizontal_space(),
                        text("Fine detail").size(9).color(th::text_muted()),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(6)
                .width(Length::Fill),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .padding([14, 16])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        });

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
                sensitivity_control,
                text("Analysis replaces detected markers. Manual markers stay in place.")
                    .size(10)
                    .color(th::text_muted()),
                row![horizontal_space(), cancel, analyse]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
            ]
            .spacing(12),
        )
        .width(Length::Fixed(460.0))
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
