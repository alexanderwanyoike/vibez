//! Truthful, non-interactive waveform for the Browser Audition footer.

use std::cell::Cell;
use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};
use vibez_core::audio_buffer::DecodedAudio;

use crate::message::Message;
use crate::theme;

pub struct BrowserWaveform {
    pub audio: Option<Arc<DecodedAudio>>,
}

pub struct BrowserWaveformState {
    cache: canvas::Cache,
    /// (audio Arc pointer, theme epoch): a palette swap must repaint
    /// the cached geometry, not just a new selection.
    fingerprint: Cell<(u64, u64)>,
}

impl Default for BrowserWaveformState {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            fingerprint: Cell::new((u64::MAX, u64::MAX)),
        }
    }
}

impl canvas::Program<Message> for BrowserWaveform {
    type State = BrowserWaveformState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let fingerprint = (
            self.audio
                .as_ref()
                .map(|audio| Arc::as_ptr(audio) as u64)
                .unwrap_or(0),
            theme::epoch(),
        );
        if state.fingerprint.get() != fingerprint {
            state.cache.clear();
            state.fingerprint.set(fingerprint);
        }

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let width = frame.width();
            let height = frame.height();
            frame.fill_rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(width, height),
                theme::display_bg(),
            );

            let middle = height / 2.0;
            let center_line = canvas::Path::line(
                iced::Point::new(0.0, middle),
                iced::Point::new(width, middle),
            );
            frame.stroke(
                &center_line,
                canvas::Stroke::default()
                    .with_color(theme::border())
                    .with_width(1.0),
            );

            let Some(audio) = &self.audio else {
                return;
            };
            let peaks = summarize_waveform(audio, width.max(1.0) as usize);
            let amplitude = (height / 2.0 - 2.0).max(1.0);
            for (x, (min, max)) in peaks.into_iter().enumerate() {
                let top = middle - max.clamp(-1.0, 1.0) * amplitude;
                let bottom = middle - min.clamp(-1.0, 1.0) * amplitude;
                frame.fill_rectangle(
                    iced::Point::new(x as f32, top.min(bottom)),
                    iced::Size::new(1.0, (bottom - top).abs().max(1.0)),
                    theme::waveform(),
                );
            }
        });

        vec![geometry]
    }
}

pub(crate) fn summarize_waveform(audio: &DecodedAudio, columns: usize) -> Vec<(f32, f32)> {
    let columns = columns.max(1);
    let frames = audio.num_frames();
    if frames == 0 || audio.num_channels() == 0 {
        return vec![(0.0, 0.0); columns];
    }

    (0..columns)
        .map(|column| {
            let start = column * frames / columns;
            let end = ((column + 1) * frames / columns).max(start + 1);
            let mut min = 0.0;
            let mut max = 0.0;
            for channel in 0..audio.num_channels() {
                let (channel_min, channel_max) = audio.peak_in_range(channel, start, end);
                min += channel_min;
                max += channel_max;
            }
            let channels = audio.num_channels() as f32;
            (min / channels, max / channels)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_summary_reflects_the_selected_audio() {
        let quiet = DecodedAudio {
            channels: vec![vec![0.1, -0.1, 0.1, -0.1]],
            sample_rate: 44_100,
        };
        let transient = DecodedAudio {
            channels: vec![vec![0.0, 1.0, 0.0, -0.8]],
            sample_rate: 44_100,
        };

        assert_ne!(
            summarize_waveform(&quiet, 4),
            summarize_waveform(&transient, 4)
        );
        assert_eq!(summarize_waveform(&transient, 4)[1], (1.0, 1.0));
    }
}
