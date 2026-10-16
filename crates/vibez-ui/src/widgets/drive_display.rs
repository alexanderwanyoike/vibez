//! The Drive's plots, both drawn from its curve: input level against output
//! level with the measured input marked, and a sine at that level before
//! and after the curve.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::drive::transfer;

use crate::message::Message;
use crate::theme as th;

const TOP_DB: f32 = 12.0;
const FLOOR_DB: f32 = -48.0;
const LABEL_W: f32 = 24.0;
const LABEL_H: f32 = 14.0;
/// The waveform shows a sine at this level when nothing is playing.
const RESTING_PEAK: f32 = 0.5;

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

fn panel(frame: &mut canvas::Frame, size: Size) {
    frame.fill(
        &canvas::Path::rounded_rectangle(Point::ORIGIN, size, 3.0.into()),
        th::bg_dark(),
    );
}

fn plot_area(size: Size) -> (Rectangle, bool) {
    let labels = size.height >= 60.0;
    let left = if labels { LABEL_W } else { 4.0 };
    let bottom = if labels { LABEL_H } else { 4.0 };
    (
        Rectangle::new(
            Point::new(left, 4.0),
            Size::new(
                (size.width - left - 6.0).max(1.0),
                (size.height - bottom - 6.0).max(1.0),
            ),
        ),
        labels,
    )
}

fn to_db(amplitude: f32) -> f32 {
    vibez_core::level::amplitude_to_db(amplitude.abs().max(1e-6))
}

/// Output level against input level, both in dB.
pub struct TransferCurve<'a> {
    pub params: &'a [f32],
    /// The measured input peak, linear; zero when nothing is playing.
    pub input_peak: f32,
    pub accent: Color,
}

impl canvas::Program<Message> for TransferCurve<'_> {
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
        panel(&mut frame, bounds.size());
        let (plot, labels) = plot_area(bounds.size());
        let span = TOP_DB - FLOOR_DB;
        let x = |db: f32| plot.x + (db.clamp(FLOOR_DB, TOP_DB) - FLOOR_DB) / span * plot.width;
        let y = |db: f32| plot.y + (TOP_DB - db.clamp(FLOOR_DB, TOP_DB)) / span * plot.height;
        for db in [12.0f32, 0.0, -12.0, -24.0, -36.0, -48.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(db), plot.y),
                    Point::new(x(db), plot.y + plot.height),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y(db)),
                    iced::alignment::Horizontal::Right,
                );
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(x(db), plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    iced::alignment::Horizontal::Center,
                );
            }
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(x(FLOOR_DB), y(FLOOR_DB)),
                Point::new(x(TOP_DB), y(TOP_DB)),
            ),
            canvas::Stroke::default()
                .with_color(th::text_muted())
                .with_width(1.0),
        );
        let output_db = |input_db: f32| {
            to_db(transfer(
                self.params,
                vibez_core::level::db_to_amplitude(input_db),
            ))
        };
        let curve = canvas::Path::new(|builder| {
            for step in 0..=96 {
                let input_db = FLOOR_DB + span * step as f32 / 96.0;
                let point = Point::new(x(input_db), y(output_db(input_db)));
                if step == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &curve,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        if self.input_peak > 0.0 {
            let input_db = to_db(self.input_peak);
            if input_db > FLOOR_DB {
                frame.fill(
                    &canvas::Path::circle(Point::new(x(input_db), y(output_db(input_db))), 4.0),
                    self.accent,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Two cycles of a sine at the measured input peak, before and after the
/// curve.
pub struct Waveform<'a> {
    pub params: &'a [f32],
    pub input_peak: f32,
    pub accent: Color,
}

impl canvas::Program<Message> for Waveform<'_> {
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
        panel(&mut frame, bounds.size());
        let (plot, labels) = plot_area(bounds.size());
        let playing = self.input_peak > vibez_core::level::db_to_amplitude(FLOOR_DB);
        let peak = if playing {
            self.input_peak
        } else {
            RESTING_PEAK
        };
        let samples: Vec<(f32, f32)> = (0..=128)
            .map(|step| {
                let input = peak * (std::f32::consts::TAU * 2.0 * step as f32 / 128.0).sin();
                (input, transfer(self.params, input))
            })
            .collect();
        let range = samples
            .iter()
            .map(|(input, output)| input.abs().max(output.abs()))
            .fold(1.0f32, f32::max);
        let x = |step: usize| plot.x + plot.width * step as f32 / 128.0;
        let y = |value: f32| plot.y + plot.height * (0.5 - 0.5 * value / range);
        for value in [range, range * 0.5, 0.0, -range * 0.5, -range] {
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
        let trace = |pick: fn(&(f32, f32)) -> f32| {
            canvas::Path::new(|builder| {
                for (step, sample) in samples.iter().enumerate() {
                    let point = Point::new(x(step), y(pick(sample)));
                    if step == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            })
        };
        frame.stroke(
            &trace(|sample| sample.0),
            canvas::Stroke::default()
                .with_color(th::text_dim())
                .with_width(1.0),
        );
        frame.stroke(
            &trace(|sample| sample.1),
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
        vec![frame.into_geometry()]
    }
}
