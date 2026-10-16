//! The Bitcrush scope: a sine at the measured input level, and what the
//! device holds of it at the current bits, rate and drive.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::bitcrush::{crush, param, BITCRUSH_PARAMS};

use crate::message::Message;
use crate::theme as th;

const LABEL_W: f32 = 24.0;
const DIVISIONS: usize = 4;
const DIVISION_MS: f32 = 1.0;
/// The sine's frequency: one cycle a division.
const TONE_HZ: f32 = 1000.0 / DIVISION_MS;
/// The sine's level when nothing is playing.
const RESTING_PEAK: f32 = 0.8;
const FLOOR_DB: f32 = -60.0;

fn grid() -> Color {
    Color {
        a: 0.35,
        ..th::border()
    }
}

fn label(
    frame: &mut canvas::Frame,
    text: String,
    at: Point,
    horizontal: iced::alignment::Horizontal,
) {
    frame.fill_text(canvas::Text {
        content: text,
        position: at,
        color: th::text_muted(),
        size: 9.0.into(),
        horizontal_alignment: horizontal,
        vertical_alignment: iced::alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

pub struct Scope<'a> {
    pub params: &'a [f32],
    pub host_rate: u32,
    /// The measured input peak, linear; zero when nothing is playing.
    pub input_peak: f32,
    pub accent: Color,
}

impl Scope<'_> {
    /// The input sine and what is held of it, one pair a host sample.
    fn samples(&self, peak: f32) -> Vec<(f32, f32)> {
        let rate = self.host_rate.max(1) as f32;
        let frames = (rate * DIVISION_MS * DIVISIONS as f32 / 1000.0).ceil() as usize;
        let divider = self
            .params
            .get(param::SR_REDUCE)
            .copied()
            .unwrap_or(BITCRUSH_PARAMS[param::SR_REDUCE].default)
            .round()
            .max(1.0) as usize;
        let mut held = 0.0;
        (0..=frames)
            .map(|frame| {
                let input = peak * (std::f32::consts::TAU * TONE_HZ * frame as f32 / rate).sin();
                if frame % divider == 0 {
                    held = crush(self.params, input);
                }
                (input, held)
            })
            .collect()
    }
}

impl canvas::Program<Message> for Scope<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
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
        let plot = Rectangle::new(
            Point::new(left, 4.0),
            Size::new(
                (bounds.width - left - 6.0).max(1.0),
                (bounds.height - 8.0).max(1.0),
            ),
        );
        let y = |value: f32| plot.y + plot.height * (0.5 - 0.5 * value.clamp(-1.0, 1.0));
        for value in [1.0f32, 0.5, 0.0, -0.5, -1.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(value)),
                    Point::new(plot.x + plot.width, y(value)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    format!("{value:.1}"),
                    Point::new(LABEL_W - 4.0, y(value)),
                    iced::alignment::Horizontal::Right,
                );
            }
        }
        for division in 1..DIVISIONS {
            let x = plot.x + plot.width * division as f32 / DIVISIONS as f32;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
        }
        let playing = self.input_peak > vibez_core::level::db_to_amplitude(FLOOR_DB);
        let samples = self.samples(if playing {
            self.input_peak
        } else {
            RESTING_PEAK
        });
        let x = |index: usize| plot.x + plot.width * index as f32 / (samples.len() - 1) as f32;
        let input = canvas::Path::new(|builder| {
            for (index, (sample, _)) in samples.iter().enumerate() {
                let point = Point::new(x(index), y(*sample));
                if index == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &input,
            canvas::Stroke::default()
                .with_color(th::text_dim())
                .with_width(1.5),
        );
        // Held values step at each sample, as the device outputs them.
        let held = canvas::Path::new(|builder| {
            for (index, (_, sample)) in samples.iter().enumerate() {
                let point = Point::new(x(index), y(*sample));
                if index == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(Point::new(point.x, y(samples[index - 1].1)));
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &held,
            canvas::Stroke::default()
                .with_color(if playing {
                    self.accent
                } else {
                    Color {
                        a: 0.5,
                        ..self.accent
                    }
                })
                .with_width(2.0),
        );
        if labels {
            label(
                &mut frame,
                format!("{DIVISION_MS:.2} ms/div"),
                Point::new(plot.x + plot.width - 4.0, plot.y + plot.height - 8.0),
                iced::alignment::Horizontal::Right,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_line_steps_once_every_divider_samples() {
        let mut params: Vec<f32> = BITCRUSH_PARAMS.iter().map(|d| d.default).collect();
        params[param::BITS] = 16.0;
        params[param::SR_REDUCE] = 4.0;
        let scope = Scope {
            params: &params,
            host_rate: 48_000,
            input_peak: 0.5,
            accent: Color::WHITE,
        };
        let samples = scope.samples(0.5);
        assert_eq!(samples.len(), 193);
        for chunk in samples.chunks(4) {
            assert!(chunk.iter().all(|(_, held)| *held == chunk[0].1));
        }
    }
}
