//! The Limiter's plots: the input over the last seconds with the reduction
//! hanging from the top and the ceiling across, and how much it takes off
//! now.

use std::time::{Duration, Instant};

use iced::alignment::Horizontal;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::domains::device_card::DynamicsReading;
use crate::domains::device_levels::ReadingHistory;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::LABEL_SIZE;

const FLOOR_DB: f32 = -48.0;
const LABEL_W: f32 = 26.0;
const LABEL_H: f32 = 14.0;
/// The reduction meter's scale, in dB taken off.
const REDUCTION_RANGE_DB: f32 = 12.0;

fn grid() -> Color {
    th::with_alpha(th::border(), 0.35)
}

fn label(frame: &mut canvas::Frame, text: String, at: Point, align: Horizontal) {
    frame.fill_text(canvas::Text {
        content: text,
        position: at,
        color: th::text_muted(),
        size: LABEL_SIZE.into(),
        horizontal_alignment: align,
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

/// The input level and the reduction over the last `span`, newest at the
/// right, with the ceiling across.
pub struct HistoryPlot<'a> {
    pub history: &'a ReadingHistory<DynamicsReading>,
    pub now: Instant,
    pub span: Duration,
    pub ceiling_db: f32,
    pub accent: Color,
}

impl canvas::Program<Message> for HistoryPlot<'_> {
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
        let labels = bounds.height >= 50.0;
        let left = if labels { LABEL_W } else { 4.0 };
        let bottom = if labels { LABEL_H } else { 4.0 };
        let plot = Rectangle::new(
            Point::new(left, 4.0),
            Size::new(
                (bounds.width - left - 6.0).max(1.0),
                (bounds.height - bottom - 6.0).max(1.0),
            ),
        );
        let y = |db: f32| plot.y + plot.height * (db.clamp(FLOOR_DB, 0.0) / FLOOR_DB);
        let span = self.span.as_secs_f32().max(0.1);
        let x = |age: f32| plot.x + plot.width * (1.0 - age / span);
        // A short plot names every other level, so the names keep apart.
        let every = if plot.height >= 90.0 { 1 } else { 2 };
        for (step, db) in [0.0f32, -12.0, -24.0, -36.0, -48.0].into_iter().enumerate() {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels && step % every == 0 {
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y(db)),
                    Horizontal::Right,
                );
            }
        }
        let seconds = span.round() as u32;
        let step = if seconds > 6 { 2 } else { 1 };
        for second in (0..=seconds).step_by(step) {
            let at = x(second as f32);
            frame.stroke(
                &canvas::Path::line(Point::new(at, plot.y), Point::new(at, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                let align = if second == 0 {
                    Horizontal::Right
                } else if second == seconds {
                    Horizontal::Left
                } else {
                    Horizontal::Center
                };
                let text = if second == 0 {
                    "0s".into()
                } else {
                    format!("-{second}s")
                };
                label(
                    &mut frame,
                    text,
                    Point::new(at, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    align,
                );
            }
        }
        let age = |at: &Instant| self.now.saturating_duration_since(*at).as_secs_f32();
        let readings: Vec<(f32, DynamicsReading)> = self
            .history
            .iter()
            .map(|(at, reading)| (age(at), *reading))
            .filter(|(age, _)| *age <= span)
            .collect();
        // The input as one filled shape through its readings, so it reads
        // the same however often they arrive.
        if readings.len() > 1 {
            let bottom = plot.y + plot.height;
            let input = canvas::Path::new(|builder| {
                builder.move_to(Point::new(x(readings[0].0), bottom));
                for (age, reading) in &readings {
                    builder.line_to(Point::new(x(*age), y(reading.input_db)));
                }
                builder.line_to(Point::new(x(readings[readings.len() - 1].0), bottom));
                builder.close();
            });
            frame.fill(&input, th::with_alpha(th::text_dim(), 0.45));
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, y(self.ceiling_db)),
                Point::new(plot.x + plot.width, y(self.ceiling_db)),
            ),
            canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &[3.0, 3.0],
                    offset: 0,
                },
                ..canvas::Stroke::default()
                    .with_color(th::with_alpha(self.accent, 0.7))
                    .with_width(1.0)
            },
        );
        if readings.len() > 1 {
            let reduction = canvas::Path::new(|builder| {
                for (index, (age, reading)) in readings.iter().enumerate() {
                    let point = Point::new(x(*age), y(-reading.reduction_db));
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });
            frame.stroke(
                &reduction,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.5),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// How much the limiter takes off now, filling down from the top.
pub struct ReductionMeter {
    pub reduction_db: f32,
    pub accent: Color,
}

impl canvas::Program<Message> for ReductionMeter {
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
        let labels = bounds.height >= 50.0;
        let bar_w = 10.0;
        let left = if labels { 18.0 } else { 0.0 };
        let bar = Rectangle::new(
            Point::new(left + 4.0, 4.0),
            Size::new(bar_w, (bounds.height - 8.0).max(1.0)),
        );
        let y =
            |db: f32| bar.y + bar.height * (db.clamp(0.0, REDUCTION_RANGE_DB) / REDUCTION_RANGE_DB);
        frame.fill_rectangle(bar.position(), bar.size(), th::bg_dark());
        frame.stroke(
            &canvas::Path::rectangle(bar.position(), bar.size()),
            canvas::Stroke::default()
                .with_color(th::border())
                .with_width(1.0),
        );
        if labels {
            let every = if bar.height >= 90.0 { 3 } else { 6 };
            for db in (0..=REDUCTION_RANGE_DB as u32).step_by(every) {
                label(
                    &mut frame,
                    db.to_string(),
                    Point::new(left, y(db as f32)),
                    Horizontal::Right,
                );
            }
        }
        if self.reduction_db > 0.01 {
            frame.fill_rectangle(
                Point::new(bar.x + 1.0, bar.y + 1.0),
                Size::new(
                    bar.width - 2.0,
                    (y(self.reduction_db) - bar.y - 1.0).max(1.0),
                ),
                self.accent,
            );
        }
        vec![frame.into_geometry()]
    }
}
