use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use vibez_core::audio_buffer::DecodedAudio;

use crate::theme;

pub struct WaveformWidget {
    pub audio: Option<Arc<DecodedAudio>>,
    pub playhead_position: f64, // 0.0..1.0
    pub cache: canvas::Cache,
}

impl Default for WaveformWidget {
    fn default() -> Self {
        Self {
            audio: None,
            playhead_position: 0.0,
            cache: canvas::Cache::new(),
        }
    }
}

impl WaveformWidget {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_audio(&mut self, audio: Option<Arc<DecodedAudio>>) {
        self.audio = audio;
        self.cache.clear();
    }

    pub fn set_playhead(&mut self, position: f64) {
        self.playhead_position = position;
    }
}

impl canvas::Program<crate::message::Message> for WaveformWidget {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let waveform = self.cache.draw(renderer, bounds.size(), |frame| {
            let w = bounds.width;
            let h = bounds.height;

            // Background
            frame.fill_rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(w, h),
                theme::BG_SURFACE,
            );

            // Center line
            let center_y = h / 2.0;
            let center_line = canvas::Path::line(
                iced::Point::new(0.0, center_y),
                iced::Point::new(w, center_y),
            );
            frame.stroke(
                &center_line,
                canvas::Stroke::default()
                    .with_color(Color {
                        a: 0.3,
                        ..theme::TEXT_DIM
                    })
                    .with_width(1.0),
            );

            // Draw waveform
            if let Some(ref audio) = self.audio {
                let num_frames = audio.num_frames();
                if num_frames == 0 {
                    return;
                }

                let pixels = w as usize;
                let half_h = h / 2.0;

                for x in 0..pixels {
                    let start = (x as f64 * num_frames as f64 / pixels as f64) as usize;
                    let end = ((x + 1) as f64 * num_frames as f64 / pixels as f64) as usize;

                    // Average across channels for display
                    let mut min_val = 0.0f32;
                    let mut max_val = 0.0f32;
                    let channels = audio.num_channels();
                    for ch in 0..channels {
                        let (ch_min, ch_max) = audio.peak_in_range(ch, start, end);
                        min_val += ch_min;
                        max_val += ch_max;
                    }
                    if channels > 0 {
                        min_val /= channels as f32;
                        max_val /= channels as f32;
                    }

                    let y_top = center_y - (max_val * half_h);
                    let y_bottom = center_y - (min_val * half_h);
                    let height = (y_bottom - y_top).max(1.0);

                    frame.fill_rectangle(
                        iced::Point::new(x as f32, y_top),
                        iced::Size::new(1.0, height),
                        theme::WAVEFORM,
                    );
                }
            }
        });

        // Playhead (drawn every frame, not cached)
        let playhead = {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            if self.audio.is_some() {
                let x = (self.playhead_position as f32) * bounds.width;
                let playhead_line = canvas::Path::line(
                    iced::Point::new(x, 0.0),
                    iced::Point::new(x, bounds.height),
                );
                frame.stroke(
                    &playhead_line,
                    canvas::Stroke::default()
                        .with_color(theme::PLAYHEAD)
                        .with_width(1.5),
                );
            }
            frame.into_geometry()
        };

        vec![waveform, playhead]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.audio.is_some() && cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<crate::message::Message>) {
        if self.audio.is_none() {
            return (canvas::event::Status::Ignored, None);
        }

        if let canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) =
            event
        {
            if let Some(pos) = cursor.position_in(bounds) {
                let normalized = (pos.x / bounds.width) as f64;
                return (
                    canvas::event::Status::Captured,
                    Some(crate::message::Message::Seek(normalized.clamp(0.0, 1.0))),
                );
            }
        }

        (canvas::event::Status::Ignored, None)
    }
}
