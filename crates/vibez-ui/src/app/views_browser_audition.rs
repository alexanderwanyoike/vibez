//! Sample-browser audition footer rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{button, canvas, column, container, row, slider, text};
use iced::{Element, Length, Theme};

use super::views_browser_style::*;
use super::*;
use crate::icons;
use crate::message::Message;
use crate::state::{AuditionMode, SampleBrowserMode};
use crate::theme as th;

impl App {
    pub(super) fn view_browser_audition_footer(&self) -> Element<'_, Message> {
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

        let waveform: Element<'_, Message> = container(
            canvas(crate::widgets::browser_waveform::BrowserWaveform {
                audio: self.state.browser.waveform_audio.clone(),
                playhead_fraction: self.state.browser.audition_playhead_fraction(),
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
            } else if self.state.browser.audition_loading
                && self.state.browser.audition_playing
                && self.state.browser.audition_playback_mode == Some(AuditionMode::Raw)
            {
                "WARPING"
            } else if self.state.browser.audition_loading {
                "PREPARING"
            } else if self.state.browser.audition_queued {
                "QUEUED"
            } else if self.state.browser.audition_playing {
                match self.state.browser.audition_playback_mode {
                    Some(AuditionMode::Raw)
                        if self.state.browser.audition_mode == AuditionMode::Warp =>
                    {
                        "WARPING"
                    }
                    Some(mode) => mode.label(),
                    None => "PLAYING",
                }
            } else if self.state.browser.waveform_error.is_some() {
                "UNAVAILABLE"
            } else if self.state.browser.waveform_audio.is_some() {
                self.state.browser.audition_mode.label()
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
                raw,
                warp,
                text(selected_label)
                    .size(10)
                    .color(th::text_dim())
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(5)
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
