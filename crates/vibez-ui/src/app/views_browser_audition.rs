//! Sample-browser audition footer rendering.
//! Split from views_browser.rs; inherent methods on [`super::App`].

use iced::widget::{button, canvas, column, container, row, slider, text};
use iced::{Element, Length, Theme};

use super::views_browser_style::*;
use super::*;
use crate::domains::browser::DragMedia;
use crate::icons;
use crate::message::Message;
use crate::state::{AuditionMode, SampleBrowserMode};
use crate::theme as th;
use crate::widgets::control_style;
use crate::widgets::device_chrome::tiny_button;
use crate::widgets::fitted_label::FittedLabel;

impl App {
    pub(super) fn view_browser_audition_footer(&self) -> Element<'_, Message> {
        let selected_local = self.selected_sample_browser_entry();
        let selected_dropbox = self.selected_dropbox_entry();
        let selected_midi = self.state.browser.mode == SampleBrowserMode::Local
            && selected_local.is_some_and(|entry| DragMedia::of(&entry.source) == DragMedia::Midi);
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
        let accent = th::accent();
        let play = tiny_button(icons::PLAY, "Preview", preview_message, None, accent);
        let stop = tiny_button(
            icons::STOP,
            "Stop preview",
            Some(Message::StopBrowserPreview),
            None,
            accent,
        );
        let switch = |label: &'static str, on: bool, message: Message| {
            button(text(label).size(10))
                .on_press(message)
                .padding([3, 8])
                .style(move |_theme: &Theme, status| button::Style {
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..control_style::styled(on.then_some(accent), status)
                })
        };
        let enabled = self.state.browser.audition_enabled;
        let follow_toggle = switch(
            if enabled { "On" } else { "Off" },
            enabled,
            Message::ToggleAuditionEnabled,
        );
        let raw = switch(
            "Raw",
            self.state.browser.audition_mode == AuditionMode::Raw,
            Message::SetAuditionMode(AuditionMode::Raw),
        );
        let warp = switch(
            "Warp",
            self.state.browser.audition_mode == AuditionMode::Warp,
            Message::SetAuditionMode(AuditionMode::Warp),
        );

        let picture: Element<'_, Message> = if selected_midi {
            let preview = self.state.browser.midi_preview.as_ref();
            canvas(crate::widgets::browser_midi_strip::BrowserMidiStrip {
                strip: preview.map(crate::domains::midi_audition::MidiPreview::strip),
                identity: preview.map_or(0, |preview| Arc::as_ptr(&preview.pattern) as u64),
                playhead_fraction: self
                    .state
                    .browser
                    .midi_audition_playhead(self.state.transport.samples_per_beat()),
            })
            .width(Length::Fill)
            .height(Length::Fixed(26.0))
            .into()
        } else {
            canvas(crate::widgets::browser_waveform::BrowserWaveform {
                audio: self.state.browser.waveform_audio.clone(),
                playhead_fraction: self.state.browser.audition_playhead_fraction(),
            })
            .width(Length::Fill)
            .height(Length::Fixed(26.0))
            .into()
        };
        let waveform: Element<'_, Message> = container(picture)
            .width(Length::Fill)
            .padding([0, 4])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::display_bg().into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
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
            } else if self.state.browser.waveform_audio.is_some()
                || self.state.browser.midi_preview.is_some()
            {
                self.state.browser.audition_mode.label()
            } else {
                "SELECT"
            })
            .size(9)
            .color(th::text_dim())
            .width(Length::Fixed(audition_status_width()))
            .wrapping(iced::widget::text::Wrapping::None),
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
                    backgrounds: (th::text_dim().into(), th::control().into()),
                    width: 4.0,
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                },
                handle: iced::widget::slider::Handle {
                    shape: iced::widget::slider::HandleShape::Circle { radius: 6.0 },
                    background: if matches!(status, iced::widget::slider::Status::Dragged) {
                        th::accent().into()
                    } else {
                        th::text().into()
                    },
                    border_width: 0.0,
                    border_color: iced::Color::TRANSPARENT,
                },
            });
        let gain_row = row![
            text("Gain").size(10).color(th::text_muted()),
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
                text("AUDITION")
                    .size(9.5)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::DEFAULT
                    })
                    .color(th::text_muted()),
                follow_toggle,
                raw,
                warp,
                self.view_audition_source_label(selected_label, selected_midi)
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
            controls,
            gain_row
        ]
        .spacing(8)
        .into();

        container(
            container(contents)
                .padding(10)
                .width(Length::Fill)
                .style(browser_footer_style),
        )
        .padding(iced::Padding {
            top: 6.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        })
        .width(Length::Fill)
        .into()
    }

    /// The selected source's name; a MIDI file's is cut to fit beside who
    /// will play it, so the header stays one line.
    fn view_audition_source_label(&self, name: String, midi: bool) -> Element<'_, Message> {
        if !midi {
            return text(name)
                .size(10)
                .color(th::text_dim())
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .wrapping(iced::widget::text::Wrapping::None)
                .into();
        }
        let player = crate::domains::midi_audition::audition_player(
            self.state.arrangement.selected_track,
            &self.state.project_tracks.tracks,
        );
        row![
            FittedLabel::name(name, 10.0, th::text_dim()).view(14.0),
            text(player)
                .size(10)
                .color(th::text_muted())
                .wrapping(iced::widget::text::Wrapping::None)
        ]
        .spacing(6)
        .width(Length::Fill)
        .align_y(iced::Alignment::Center)
        .into()
    }
}

/// Wide enough for the longest status shown beside a waveform or strip, so
/// the picture never moves when the status changes. UNAVAILABLE is longer,
/// but it has no picture to move.
fn audition_status_width() -> f32 {
    crate::widgets::device_controls::text_width("PREPARING", 9.0, iced::Font::DEFAULT).ceil()
}
