//! Sample-browser audition footer rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, row, slider, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::{AuditionMode, SampleBrowserMode};
use crate::theme as th;
use vibez_engine::commands::AuditionSync;

use super::views_browser_style::*;
use super::*;

impl App {
    pub(super) fn view_browser_audition_footer(&self) -> Element<'_, Message> {
        let compact = self
            .state
            .browser
            .effective_dock_width(self.state.view.window_width)
            < 360.0;
        let selected_local = self.selected_sample_browser_entry();
        let selected_dropbox = self.selected_dropbox_entry();
        let selected_label = match self.state.browser.mode {
            SampleBrowserMode::Local => selected_local
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| "No source selected".into()),
            SampleBrowserMode::Remote => selected_dropbox
                .as_ref()
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| "No source selected".into()),
        };

        let preview_message = match self.state.browser.mode {
            SampleBrowserMode::Local => {
                selected_local.map(|entry| Message::PreviewLocalEntry(entry.source.clone()))
            }
            SampleBrowserMode::Remote => selected_dropbox
                .as_ref()
                .filter(|entry| entry.is_supported_audio())
                .map(|entry| Message::DropboxPreview(entry.clone())),
        };
        let mut play = button(icons::icon(icons::PLAY).size(12).color(th::text_dim()))
            .padding([6, 8])
            .style(browser_transport_button_style);
        if let Some(message) = preview_message {
            play = play.on_press(message);
        }
        let stop = button(icons::icon(icons::STOP).size(11).color(th::text_dim()))
            .on_press(Message::StopBrowserPreview)
            .padding([6, 8])
            .style(browser_transport_button_style);
        let enabled = self.state.browser.audition_enabled;
        let follow_toggle = button(
            text(if enabled { "ENABLED ON" } else { "ENABLED OFF" })
                .size(9)
                .color(if enabled {
                    th::accent()
                } else {
                    th::text_dim()
                }),
        )
        .on_press(Message::ToggleAuditionEnabled)
        .padding([2, 4])
        .style(browser_utility_action_style);
        let import_label = match self.state.browser.audition_import_input() {
            Some(input) if input.mode == AuditionMode::Raw => "IMPORT RAW".to_string(),
            Some(input) => format!("IMPORT WARP {:.1}", input.source_bpm.unwrap_or_default()),
            None => "IMPORT BLOCKED".to_string(),
        };

        let raw_active = self.state.browser.audition_mode == AuditionMode::Raw;
        let raw = button(text("RAW").size(9))
            .on_press(Message::SetAuditionMode(AuditionMode::Raw))
            .padding([2, 5])
            .style(move |_theme: &Theme, status| browser_place_button_style(raw_active, status));
        let warp_active = self.state.browser.audition_mode == AuditionMode::Warp;
        let warp = button(text("WARP").size(9))
            .on_press(Message::SetAuditionMode(AuditionMode::Warp))
            .padding([2, 5])
            .style(move |_theme: &Theme, status| browser_place_button_style(warp_active, status));
        let sync_button = |label, value| {
            let active = self.state.browser.audition_sync == value;
            button(text(label).size(9))
                .on_press(Message::SetAuditionSync(value))
                .padding([2, 4])
                .style(move |_theme: &Theme, status| browser_place_button_style(active, status))
        };
        let looped = self.state.browser.audition_loop;
        let loop_toggle = button(text(if looped { "LOOP ON" } else { "LOOP OFF" }).size(9))
            .on_press(Message::ToggleAuditionLoop)
            .padding([2, 4])
            .style(move |_theme: &Theme, status| browser_place_button_style(looped, status));

        let bpm_input = text_input("BPM", &self.state.browser.audition_bpm_edit)
            .on_input(Message::AuditionBpmEditChanged)
            .on_submit(Message::ConfirmAuditionBpm)
            .size(10)
            .padding([3, 5])
            .width(Length::Fixed(if compact { 54.0 } else { 62.0 }))
            .style(browser_compact_input_style);
        let confirm_bpm = button(text(if compact { "USE" } else { "USE SOURCE" }).size(9))
            .on_press(Message::ConfirmAuditionBpm)
            .padding([3, 5])
            .style(browser_utility_action_style);
        let automatic_bpm = self
            .state
            .browser
            .audition_bpm_suggestion
            .zip(self.state.browser.audition_bpm_confidence)
            .is_some_and(|(suggestion, confidence)| {
                confidence >= self.state.warp_confidence_threshold
                    && self.state.browser.audition_bpm_confirmed == Some(suggestion)
            });
        let bpm_controls: Element<'_, Message> = if automatic_bpm {
            text("AUTO").size(9).color(th::accent()).into()
        } else {
            row![bpm_input, confirm_bpm]
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .into()
        };
        let project_bpm = self.state.transport.bpm;
        let bpm_state = if self.state.browser.audition_bpm_detecting {
            format!("DETECTING SOURCE → {project_bpm:.0}")
        } else if let Some(confirmed) = self.state.browser.audition_bpm_confirmed {
            if compact {
                format!("{confirmed:.1} → {project_bpm:.0}")
            } else {
                format!("SOURCE {confirmed:.1} → PROJECT {project_bpm:.0}")
            }
        } else if let Some(suggestion) = self.state.browser.audition_bpm_suggestion {
            let low = self
                .state
                .browser
                .audition_bpm_confidence
                .is_some_and(|confidence| confidence < self.state.warp_confidence_threshold);
            if low {
                format!("LOW {suggestion:.1} → PROJECT {project_bpm:.0}")
            } else {
                format!("SOURCE {suggestion:.1} → PROJECT {project_bpm:.0}")
            }
        } else {
            format!("SOURCE NEEDED → PROJECT {project_bpm:.0}")
        };

        let waveform: Element<'_, Message> = container(
            canvas(crate::widgets::browser_waveform::BrowserWaveform {
                audio: self.state.browser.waveform_audio.clone(),
            })
            .width(Length::Fill)
            .height(Length::Fixed(26.0)),
        )
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            border: iced::Border {
                color: th::divider(),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into();

        let controls = row![
            play,
            stop,
            text(if self.state.browser.remote.preview_in_progress {
                "FETCHING"
            } else if self.state.browser.audition_loading {
                "PREPARING"
            } else if self.state.browser.audition_queued {
                "QUEUED"
            } else if self.state.browser.audition_playing {
                "PLAYING"
            } else if self.state.browser.waveform_error.is_some() {
                "UNAVAILABLE"
            } else if self.state.browser.waveform_audio.is_some() {
                match self.state.browser.audition_mode {
                    AuditionMode::Raw => "RAW",
                    AuditionMode::Warp if self.state.browser.audition_bpm_confirmed.is_some() => {
                        "WARP READY"
                    }
                    AuditionMode::Warp => "BPM NEEDED",
                }
            } else {
                "SELECT"
            })
            .size(9)
            .color(th::text_dim()),
            waveform
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center);
        let gain = self.state.browser.audition_gain;
        let gain_slider = slider(0.0..=2.0, gain, Message::SetAuditionGain)
            .step(0.01_f32)
            .width(Length::Fill)
            .style(|_theme: &Theme, status| iced::widget::slider::Style {
                rail: iced::widget::slider::Rail {
                    backgrounds: (th::accent_dim().into(), th::divider().into()),
                    width: 2.0,
                    border: iced::Border::default(),
                },
                handle: iced::widget::slider::Handle {
                    shape: iced::widget::slider::HandleShape::Rectangle {
                        width: 6,
                        border_radius: 0.0.into(),
                    },
                    background: if matches!(status, iced::widget::slider::Status::Dragged) {
                        th::accent().into()
                    } else {
                        th::text_dim().into()
                    },
                    border_width: 0.0,
                    border_color: iced::Color::TRANSPARENT,
                },
            });
        let gain_row = row![
            text("GAIN").size(9).color(th::text_muted()),
            gain_slider,
            text(audition_gain_label(gain))
                .size(9)
                .color(th::text_dim())
                .width(Length::Fixed(48.0))
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let contents: Element<'_, Message> = column![
            row![
                text("AUDITION").size(9).color(th::text_muted()),
                follow_toggle,
                text(import_label).size(9).color(th::text_dim()),
                text(selected_label)
                    .size(10)
                    .color(th::text_dim())
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
            row![
                text("MODE").size(9).color(th::text_muted()),
                raw,
                warp,
                text("SYNC").size(9).color(th::text_muted()),
                sync_button("OFF", AuditionSync::Off),
                sync_button("BEAT", AuditionSync::Beat),
                sync_button("BAR", AuditionSync::Bar),
                horizontal_space(),
                loop_toggle,
            ]
            .spacing(3)
            .align_y(iced::Alignment::Center),
            row![
                text(if compact { "SRC" } else { "SOURCE BPM" })
                    .size(9)
                    .color(th::text_muted()),
                bpm_controls,
                text(bpm_state)
                    .size(9)
                    .color(if self.state.browser.audition_bpm_confirmed.is_some() {
                        th::accent()
                    } else {
                        th::text_dim()
                    })
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
            controls,
            gain_row
        ]
        .spacing(5)
        .into();

        container(contents)
            .padding([7, 9])
            .width(Length::Fill)
            .style(browser_footer_style)
            .into()
    }
}
