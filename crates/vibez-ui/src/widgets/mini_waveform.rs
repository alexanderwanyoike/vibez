//! Small visual displays for device cards: sample waveforms
//! (sampler, drum pads), an oscillator scope, and an ADSR envelope
//! curve. Modeled on Ableton's Simpler/Operator displays: a device
//! should show what it will sound like, not just knobs.

use std::cell::Cell;
use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use vibez_core::audio_buffer::DecodedAudio;

/// Each column's lowest and highest sample when `samples` is drawn
/// `columns` wide.
pub fn column_peaks(samples: &[f32], columns: usize) -> impl Iterator<Item = (f32, f32)> + '_ {
    let per_column = (samples.len() / columns.max(1)).max(1);
    (0..columns)
        .map(move |col| col * per_column)
        .take_while(move |begin| *begin < samples.len())
        .map(move |begin| {
            let end = (begin + per_column).min(samples.len());
            // Stride within the column so huge files stay cheap.
            let stride = ((end - begin) / 64).max(1);
            samples[begin..end]
                .iter()
                .step_by(stride)
                .fold((f32::MAX, f32::MIN), |(min, max), s| {
                    (min.min(*s), max.max(*s))
                })
        })
}

/// Filled min/max waveform of a loaded sample, with an optional
/// highlighted playback region (drum pad start/end trim). Rendering
/// is cached and invalidated by a fingerprint of the audio pointer
/// and region, so the 60fps UI tick does not re-scan the sample.
pub struct MiniWaveform {
    pub audio: Option<Arc<DecodedAudio>>,
    pub color: Color,
    /// Normalized playback region (start, end); audio outside it is
    /// dimmed and marker lines are drawn at the edges.
    pub region: Option<(f32, f32)>,
}

pub struct MiniWaveformState {
    cache: canvas::Cache,
    /// (content hash, theme epoch): a palette swap must repaint the
    /// cached geometry, not just a new sample or region.
    fingerprint: Cell<(u64, u64)>,
}

impl Default for MiniWaveformState {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            fingerprint: Cell::new((u64::MAX, u64::MAX)),
        }
    }
}

impl MiniWaveform {
    fn fingerprint(&self) -> (u64, u64) {
        let ptr = self
            .audio
            .as_ref()
            .map(|a| Arc::as_ptr(a) as u64)
            .unwrap_or(0);
        let (s, e) = self.region.unwrap_or((0.0, 1.0));
        let content = ptr ^ ((s.to_bits() as u64) << 32) ^ (e.to_bits() as u64);
        (content, theme::epoch())
    }
}

impl canvas::Program<Message> for MiniWaveform {
    type State = MiniWaveformState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let fp = self.fingerprint();
        if state.fingerprint.get() != fp {
            state.cache.clear();
            state.fingerprint.set(fp);
        }

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let w = frame.width();
            let h = frame.height();

            // Panel background.
            let bg = canvas::Path::rounded_rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(w, h),
                3.0.into(),
            );
            frame.fill(&bg, theme::display_bg());

            let Some(audio) = &self.audio else {
                // Empty state: flat center line.
                let line = canvas::Path::line(
                    iced::Point::new(4.0, h / 2.0),
                    iced::Point::new(w - 4.0, h / 2.0),
                );
                frame.stroke(
                    &line,
                    canvas::Stroke::default()
                        .with_color(theme::border())
                        .with_width(1.0),
                );
                return;
            };

            let samples = &audio.channels[0];
            if samples.is_empty() {
                return;
            }
            let (region_start, region_end) = self.region.unwrap_or((0.0, 1.0));
            let mid = h / 2.0;
            let usable_h = h / 2.0 - 2.0;
            let columns = (w as usize).saturating_sub(4).max(1);

            for (col, (min, max)) in column_peaks(samples, columns).enumerate() {
                let pos = col as f32 / columns as f32;
                let in_region = pos >= region_start && pos <= region_end;
                let color = if in_region {
                    self.color
                } else {
                    Color {
                        a: 0.22,
                        ..self.color
                    }
                };
                let x = 2.0 + col as f32;
                let top = mid - max.clamp(-1.0, 1.0) * usable_h;
                let bottom = mid - min.clamp(-1.0, 1.0) * usable_h;
                frame.fill_rectangle(
                    iced::Point::new(x, top.min(bottom)),
                    iced::Size::new(1.0, (bottom - top).abs().max(1.0)),
                    color,
                );
            }

            // Region edge markers.
            if let Some((s, e)) = self.region {
                for pos in [s, e] {
                    let x = 2.0 + pos.clamp(0.0, 1.0) * (w - 4.0);
                    let line =
                        canvas::Path::line(iced::Point::new(x, 2.0), iced::Point::new(x, h - 2.0));
                    frame.stroke(
                        &line,
                        canvas::Stroke::default()
                            .with_color(theme::text_dim())
                            .with_width(1.0),
                    );
                }
            }
        });

        vec![geometry]
    }
}
