//! The Phaser's plots, both drawn from its DSP: the gain across the
//! spectrum at either end of the sweep, and the LFO that moves between them.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::phaser::{lfo, param, response_db, PHASER_PARAMS, SYNC_BEATS};

use crate::message::Message;
use crate::spectrum::{freq_to_norm, norm_to_freq};
use crate::theme as th;

const TOP_DB: f32 = 12.0;
const FLOOR_DB: f32 = -24.0;
const LABEL_W: f32 = 26.0;
const LABEL_H: f32 = 14.0;
const DISPLAY_RATE: f32 = 48_000.0;
/// Every 1-2-5 frequency gets a label from this plot width.
const WIDE_AXIS_W: f32 = 360.0;

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(PHASER_PARAMS[index].default)
}

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

/// Seconds as the concept writes them: no more digits than the span needs,
/// and the unit only at the end of the axis.
fn time_label(seconds: f32, last: bool) -> String {
    let digits = if seconds == 0.0 || seconds >= 10.0 {
        0
    } else if seconds >= 1.0 {
        1
    } else {
        2
    };
    let unit = if last { " s" } else { "" };
    format!("{seconds:.digits$}{unit}")
}

fn panel(frame: &mut canvas::Frame, size: Size) {
    frame.fill(
        &canvas::Path::rounded_rectangle(Point::ORIGIN, size, 3.0.into()),
        th::bg_dark(),
    );
}

fn area(size: Size) -> (Rectangle, bool) {
    let labels = size.height >= 60.0;
    let left = if labels { LABEL_W } else { 4.0 };
    let bottom = if labels { LABEL_H } else { 4.0 };
    (
        Rectangle::new(
            Point::new(left, 6.0),
            Size::new(
                (size.width - left - 8.0).max(1.0),
                (size.height - bottom - 8.0).max(1.0),
            ),
        ),
        labels,
    )
}

/// Gain against frequency with the LFO at the bottom of its sweep, and in
/// grey at the top.
pub struct ResponsePlot<'a> {
    pub params: &'a [f32],
    pub accent: Color,
}

impl canvas::Program<Message> for ResponsePlot<'_> {
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
        let (plot, labels) = area(bounds.size());
        let y = |db: f32| {
            plot.y + plot.height * (TOP_DB - db.clamp(FLOOR_DB, TOP_DB)) / (TOP_DB - FLOOR_DB)
        };
        for db in [12.0f32, 0.0, -12.0, -24.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
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
            }
        }
        let named: &[f32] = if plot.width >= WIDE_AXIS_W {
            &[
                20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
            ]
        } else {
            &[20.0, 100.0, 1_000.0, 10_000.0, 20_000.0]
        };
        for &frequency in named {
            let x = plot.x + freq_to_norm(frequency) * plot.width;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                let text = if frequency >= 1_000.0 {
                    format!("{:.0}k", frequency / 1_000.0)
                } else {
                    format!("{frequency:.0}")
                };
                label(
                    &mut frame,
                    text,
                    Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    iced::alignment::Horizontal::Center,
                );
            }
        }
        let curve = |lfo: f32| {
            canvas::Path::new(|builder| {
                for step in 0..=160 {
                    let position = step as f32 / 160.0;
                    let db = response_db(self.params, lfo, norm_to_freq(position), DISPLAY_RATE);
                    let point = Point::new(plot.x + position * plot.width, y(db));
                    if step == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            })
        };
        frame.stroke(
            &curve(1.0),
            canvas::Stroke::default()
                .with_color(th::text_muted())
                .with_width(1.5),
        );
        frame.stroke(
            &curve(0.0),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        vec![frame.into_geometry()]
    }
}

/// Two cycles of the LFO from -1 to 1, the right channel's offset dimmed
/// beside it.
pub struct LfoPlot<'a> {
    pub params: &'a [f32],
    pub bpm: f64,
    pub accent: Color,
}

impl LfoPlot<'_> {
    fn rate_hz(&self) -> f32 {
        let sync = value(self.params, param::SYNC).round() as usize;
        if sync == 0 {
            value(self.params, param::RATE)
        } else {
            (self.bpm.max(1.0) / 60.0 / SYNC_BEATS[sync - 1]) as f32
        }
    }
}

impl canvas::Program<Message> for LfoPlot<'_> {
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
        let (plot, labels) = area(bounds.size());
        let seconds = 2.0 / self.rate_hz().max(0.01);
        let shape = value(self.params, param::SHAPE);
        let y = |value: f32| plot.y + plot.height * 0.5 * (1.0 - value);
        for level in [1.0f32, 0.0, -1.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(level)),
                    Point::new(plot.x + plot.width, y(level)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    format!("{level:.0}"),
                    Point::new(LABEL_W - 4.0, y(level)),
                    iced::alignment::Horizontal::Right,
                );
            }
        }
        if labels {
            for (position, horizontal) in [
                (0.0f32, iced::alignment::Horizontal::Left),
                (0.5, iced::alignment::Horizontal::Center),
                (1.0, iced::alignment::Horizontal::Right),
            ] {
                label(
                    &mut frame,
                    time_label(seconds * position, position == 1.0),
                    Point::new(
                        plot.x + plot.width * position,
                        plot.y + plot.height + LABEL_H / 2.0 + 1.0,
                    ),
                    horizontal,
                );
            }
        }
        let trace = |offset: f32| {
            canvas::Path::new(|builder| {
                for step in 0..=128 {
                    let position = step as f32 / 128.0;
                    let point = Point::new(
                        plot.x + position * plot.width,
                        y(2.0 * lfo(shape, 2.0 * position + offset) - 1.0),
                    );
                    if step == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            })
        };
        let offset = value(self.params, param::STEREO_PHASE) / 360.0;
        if offset > 0.0 {
            frame.stroke(
                &trace(offset),
                canvas::Stroke::default()
                    .with_color(Color {
                        a: 0.45,
                        ..self.accent
                    })
                    .with_width(1.5),
            );
        }
        frame.stroke(
            &trace(0.0),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        vec![frame.into_geometry()]
    }
}
