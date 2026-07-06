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
    fingerprint: Cell<u64>,
}

impl Default for MiniWaveformState {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            fingerprint: Cell::new(u64::MAX),
        }
    }
}

impl MiniWaveform {
    fn fingerprint(&self) -> u64 {
        let ptr = self
            .audio
            .as_ref()
            .map(|a| Arc::as_ptr(a) as u64)
            .unwrap_or(0);
        let (s, e) = self.region.unwrap_or((0.0, 1.0));
        ptr ^ ((s.to_bits() as u64) << 32) ^ (e.to_bits() as u64)
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
            frame.fill(
                &bg,
                Color {
                    r: 0.07,
                    g: 0.07,
                    b: 0.07,
                    a: 1.0,
                },
            );

            let Some(audio) = &self.audio else {
                // Empty state: flat center line.
                let line = canvas::Path::line(
                    iced::Point::new(4.0, h / 2.0),
                    iced::Point::new(w - 4.0, h / 2.0),
                );
                frame.stroke(
                    &line,
                    canvas::Stroke::default()
                        .with_color(theme::BORDER)
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
            let per_column = (samples.len() / columns).max(1);

            for col in 0..columns {
                let begin = col * per_column;
                if begin >= samples.len() {
                    break;
                }
                let end = ((col + 1) * per_column).min(samples.len());
                let mut min = f32::MAX;
                let mut max = f32::MIN;
                // Stride within the column so huge files stay cheap.
                let stride = ((end - begin) / 64).max(1);
                let mut i = begin;
                while i < end {
                    let s = samples[i];
                    min = min.min(s);
                    max = max.max(s);
                    i += stride;
                }
                if min > max {
                    continue;
                }
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
                            .with_color(theme::TEXT_DIM)
                            .with_width(1.0),
                    );
                }
            }
        });

        vec![geometry]
    }
}

/// One analytic cycle of the synth's selected oscillator shape.
pub struct OscScope {
    pub waveform_index: usize,
    pub color: Color,
}

impl canvas::Program<Message> for OscScope {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = frame.width();
        let h = frame.height();

        let bg =
            canvas::Path::rounded_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), 3.0.into());
        frame.fill(
            &bg,
            Color {
                r: 0.07,
                g: 0.07,
                b: 0.07,
                a: 1.0,
            },
        );

        let mid = h / 2.0;
        let amp = h / 2.0 - 4.0;
        let points = 96;
        let shape = |phase: f32| -> f32 {
            match self.waveform_index {
                0 => (phase * std::f32::consts::TAU).sin(),
                1 => 1.0 - 2.0 * phase, // saw
                2 => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                _ => {
                    // triangle
                    if phase < 0.25 {
                        4.0 * phase
                    } else if phase < 0.75 {
                        2.0 - 4.0 * phase
                    } else {
                        4.0 * phase - 4.0
                    }
                }
            }
        };

        let path = canvas::Path::new(|b| {
            for i in 0..=points {
                let phase = i as f32 / points as f32;
                let x = 3.0 + phase * (w - 6.0);
                let y = mid - shape(phase) * amp;
                if i == 0 {
                    b.move_to(iced::Point::new(x, y));
                } else {
                    b.line_to(iced::Point::new(x, y));
                }
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.color)
                .with_width(1.5),
        );

        vec![frame.into_geometry()]
    }
}

/// ADSR envelope curve: attack rise, decay to the sustain plateau,
/// release tail. Time segments share the width proportionally with a
/// fixed plateau so short envelopes stay readable.
pub struct AdsrScope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub color: Color,
}

impl canvas::Program<Message> for AdsrScope {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = frame.width();
        let h = frame.height();

        let bg =
            canvas::Path::rounded_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), 3.0.into());
        frame.fill(
            &bg,
            Color {
                r: 0.07,
                g: 0.07,
                b: 0.07,
                a: 1.0,
            },
        );

        let pad = 3.0;
        let top = pad;
        let bottom = h - pad;
        let usable_w = w - pad * 2.0;
        let sustain_y = bottom - self.sustain.clamp(0.0, 1.0) * (bottom - top);

        // Time-proportional widths with a fixed sustain plateau.
        let total_t = (self.attack + self.decay + self.release).max(0.001);
        let plateau = usable_w * 0.22;
        let time_w = usable_w - plateau;
        let ax = pad + time_w * (self.attack / total_t);
        let dx = ax + time_w * (self.decay / total_t);
        let sx = dx + plateau;

        let path = canvas::Path::new(|b| {
            b.move_to(iced::Point::new(pad, bottom));
            b.line_to(iced::Point::new(ax, top));
            b.line_to(iced::Point::new(dx, sustain_y));
            b.line_to(iced::Point::new(sx, sustain_y));
            b.line_to(iced::Point::new(pad + usable_w, bottom));
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.color)
                .with_width(1.5),
        );

        vec![frame.into_geometry()]
    }
}
