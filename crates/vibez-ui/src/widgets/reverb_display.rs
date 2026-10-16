//! The Reverb's impulse response: the device itself rendered on a single
//! click at a low preview rate, its level every few milliseconds, against
//! the 60 dB line its decay time promises.

use std::cell::RefCell;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::effect::AudioEffect;
use vibez_dsp::reverb::{decay_seconds, param, ReverbEffect, REVERB_PARAMS};

use crate::message::Message;
use crate::theme as th;

const PREVIEW_RATE: f32 = 22_050.0;
const WINDOW_SECONDS: f32 = 0.01;
const FLOOR_DB: f32 = -60.0;
const MIN_SPAN: f32 = 3.0;
const MAX_SPAN: f32 = 6.0;
const LABEL_W: f32 = 30.0;
const LABEL_H: f32 = 14.0;

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(REVERB_PARAMS[index].default)
}

/// How long the plot runs: the pre-delay and decay, within bounds.
pub fn span_seconds(params: &[f32], bpm: f64) -> f32 {
    (predelay_seconds(params, bpm) + decay_seconds(value(params, param::ROOM_SIZE)) * 1.1)
        .clamp(MIN_SPAN, MAX_SPAN)
}

pub fn predelay_seconds(params: &[f32], bpm: f64) -> f32 {
    let sync = value(params, param::PREDELAY_SYNC).round() as usize;
    if sync == 0 {
        value(params, param::PREDELAY_MS) / 1000.0
    } else {
        (vibez_dsp::reverb::PREDELAY_SYNC_BEATS[sync - 1] * 60.0 / bpm.max(1.0)) as f32
    }
}

/// The wet response to a click, in dB below its loudest moment, one value
/// every [`WINDOW_SECONDS`].
pub fn impulse_levels(params: &[f32], bpm: f64) -> Vec<f32> {
    let mut reverb = ReverbEffect::new(PREVIEW_RATE);
    for index in 0..param::COUNT {
        reverb.set_param(index, value(params, index));
    }
    // Wet only, and never frozen: a frozen tank lets no click in.
    reverb.set_param(param::MIX, 1.0);
    reverb.set_param(param::FREEZE, 0.0);
    reverb.set_audio_context(vibez_core::audio_context::DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: PREVIEW_RATE as u32,
        bpm,
        playing: false,
    });
    let window = (PREVIEW_RATE * WINDOW_SECONDS) as usize;
    let windows = (span_seconds(params, bpm) / WINDOW_SECONDS) as usize;
    let mut buffer = vec![0.0f32; window * 2];
    let mut levels = Vec::with_capacity(windows);
    for index in 0..windows {
        buffer.fill(0.0);
        if index == 0 {
            buffer[0] = 1.0;
            buffer[1] = 1.0;
        }
        reverb.process(&mut buffer, 2);
        let power = buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32;
        levels.push(10.0 * power.max(1e-20).log10());
    }
    let peak = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    levels.iter().map(|level| level - peak).collect()
}

pub struct ImpulsePlot<'a> {
    pub params: &'a [f32],
    pub bpm: f64,
    pub accent: Color,
}

/// The settings and tempo a response was worked out for, and the response.
type Response = (Vec<f32>, f64, Vec<f32>);

/// The last response worked out, kept until a setting changes.
#[derive(Default)]
pub struct PlotState {
    cached: RefCell<Option<Response>>,
}

impl PlotState {
    fn levels(&self, params: &[f32], bpm: f64) -> Vec<f32> {
        let mut key = params.to_vec();
        if let Some(freeze) = key.get_mut(param::FREEZE) {
            *freeze = 0.0;
        }
        let mut cached = self.cached.borrow_mut();
        match cached.as_ref() {
            Some((params, cached_bpm, levels)) if *params == key && *cached_bpm == bpm => {
                levels.clone()
            }
            _ => {
                let levels = impulse_levels(&key, bpm);
                *cached = Some((key, bpm, levels.clone()));
                levels
            }
        }
    }
}

impl canvas::Program<Message> for ImpulsePlot<'_> {
    type State = PlotState;

    fn draw(
        &self,
        state: &PlotState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill(
            &canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 3.0.into()),
            th::bg_dark(),
        );
        let labels = bounds.height >= 60.0;
        let left = if labels { LABEL_W } else { 4.0 };
        let bottom = if labels { LABEL_H } else { 4.0 };
        let plot = Rectangle::new(
            Point::new(left, 6.0),
            Size::new(
                (bounds.width - left - 8.0).max(1.0),
                (bounds.height - bottom - 8.0).max(1.0),
            ),
        );
        let span = span_seconds(self.params, self.bpm);
        let x = |seconds: f32| plot.x + plot.width * (seconds / span).clamp(0.0, 1.0);
        let y = |db: f32| plot.y + plot.height * (db.clamp(FLOOR_DB, 0.0) / FLOOR_DB);
        let grid = Color {
            a: 0.35,
            ..th::border()
        };
        let label = |frame: &mut canvas::Frame,
                     text: String,
                     at: Point,
                     horizontal: iced::alignment::Horizontal,
                     color: Color| {
            frame.fill_text(canvas::Text {
                content: text,
                position: at,
                color,
                size: 9.0.into(),
                horizontal_alignment: horizontal,
                vertical_alignment: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        };
        for db in [0.0f32, -20.0, -40.0, -60.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
            if labels {
                let text = if db == 0.0 {
                    "0 dB".to_string()
                } else {
                    format!("{db:.0}")
                };
                label(
                    &mut frame,
                    text,
                    Point::new(LABEL_W - 4.0, y(db)),
                    iced::alignment::Horizontal::Right,
                    th::text_muted(),
                );
            }
        }
        let predelay = predelay_seconds(self.params, self.bpm);
        let mut second = 0.0f32;
        while second <= span + 1e-3 {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(second), plot.y),
                    Point::new(x(second), plot.y + plot.height),
                ),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
            // A pre-delay's own readout takes the place of the zero.
            if labels && !(second == 0.0 && predelay > 0.0) {
                label(
                    &mut frame,
                    format!("{second:.1}"),
                    Point::new(x(second), plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    iced::alignment::Horizontal::Center,
                    th::text_muted(),
                );
            }
            second += 0.5;
        }
        let levels = state.levels(self.params, self.bpm);
        if levels.len() > 1 {
            let at = |index: usize| index as f32 * WINDOW_SECONDS;
            let area = canvas::Path::new(|builder| {
                builder.move_to(Point::new(plot.x, plot.y + plot.height));
                for (index, level) in levels.iter().enumerate() {
                    builder.line_to(Point::new(x(at(index)), y(*level)));
                }
                builder.line_to(Point::new(x(at(levels.len() - 1)), plot.y + plot.height));
                builder.close();
            });
            frame.fill(
                &area,
                Color {
                    a: 0.22,
                    ..self.accent
                },
            );
            let line = canvas::Path::new(|builder| {
                for (index, level) in levels.iter().enumerate() {
                    let point = Point::new(x(at(index)), y(*level));
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.5),
            );
        }
        let decay = decay_seconds(value(self.params, param::ROOM_SIZE));
        frame.stroke(
            &canvas::Path::line(
                Point::new(x(predelay), y(0.0)),
                Point::new(x(predelay + decay), y(FLOOR_DB)),
            ),
            canvas::Stroke::default()
                .with_color(th::text_dim())
                .with_width(1.0),
        );
        if predelay > 0.0 {
            let marker = x(predelay);
            let mut dash = plot.y;
            while dash < plot.y + plot.height {
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(marker, dash),
                        Point::new(marker, (dash + 3.0).min(plot.y + plot.height)),
                    ),
                    canvas::Stroke::default()
                        .with_color(th::text())
                        .with_width(1.0),
                );
                dash += 6.0;
            }
            if labels {
                label(
                    &mut frame,
                    format!("{predelay:.3} s"),
                    Point::new(marker + 3.0, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    iced::alignment::Horizontal::Left,
                    self.accent,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(edits: &[(usize, f32)]) -> Vec<f32> {
        let mut params: Vec<f32> = REVERB_PARAMS.iter().map(|d| d.default).collect();
        for &(index, value) in edits {
            params[index] = value;
        }
        params
    }

    #[test]
    fn a_bigger_room_keeps_its_tail_longer() {
        let at_one_second =
            |room: f32| impulse_levels(&params(&[(param::ROOM_SIZE, room)]), 120.0)[100];
        assert!(at_one_second(0.9) > at_one_second(0.3) + 10.0);
    }

    #[test]
    fn the_tail_starts_after_the_pre_delay() {
        let levels = impulse_levels(&params(&[(param::PREDELAY_MS, 200.0)]), 120.0);
        assert!(levels[..15].iter().all(|level| *level < -80.0));
        assert!(levels[25..40].iter().any(|level| *level > -10.0));
    }

    #[test]
    fn the_plot_spans_the_decay_within_its_bounds() {
        assert_eq!(
            span_seconds(&params(&[(param::ROOM_SIZE, 0.0)]), 120.0),
            MIN_SPAN
        );
        assert_eq!(
            span_seconds(&params(&[(param::ROOM_SIZE, 1.0)]), 120.0),
            MAX_SPAN
        );
    }
}
