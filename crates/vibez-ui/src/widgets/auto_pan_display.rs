//! The Auto-Pan's plots: each side's gain over two LFO cycles, drawn from
//! the device's own gain law, and where its output sits between the
//! speakers right now.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::auto_pan::{gains_at, rate_hz};

use crate::message::Message;
use crate::theme as th;

const TOP_DB: f32 = 6.0;
const FLOOR_DB: f32 = -24.0;
const LABEL_W: f32 = 26.0;
const LABEL_H: f32 = 14.0;
const STEPS: usize = 200;

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
    color: Color,
) {
    frame.fill_text(canvas::Text {
        content: text,
        position: at,
        color,
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

/// Seconds with no more digits than the span needs, the unit only at the
/// end of the axis.
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

pub struct GainPlot<'a> {
    pub params: &'a [f32],
    pub bpm: f64,
    pub accent: Color,
}

impl canvas::Program<Message> for GainPlot<'_> {
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
        let y = |gain: f32| {
            let db = 20.0 * gain.max(1e-6).log10();
            plot.y + plot.height * (TOP_DB - db.clamp(FLOOR_DB, TOP_DB)) / (TOP_DB - FLOOR_DB)
        };
        for db in [6.0f32, 0.0, -12.0, -24.0] {
            let at = y(vibez_core::level::db_to_amplitude(db));
            frame.stroke(
                &canvas::Path::line(Point::new(plot.x, at), Point::new(plot.x + plot.width, at)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                let text = if db > 0.0 {
                    format!("+{db:.0}")
                } else {
                    format!("{db:.0}")
                };
                label(
                    &mut frame,
                    text,
                    Point::new(LABEL_W - 4.0, at),
                    iced::alignment::Horizontal::Right,
                    th::text_muted(),
                );
            }
        }
        let seconds = (2.0 / rate_hz(self.params, self.bpm).max(0.01)) as f32;
        for (position, horizontal) in [
            (0.0f32, iced::alignment::Horizontal::Left),
            (0.5, iced::alignment::Horizontal::Center),
            (1.0, iced::alignment::Horizontal::Right),
        ] {
            let x = plot.x + plot.width * position;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    time_label(seconds * position, position == 1.0),
                    Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    horizontal,
                    th::text_muted(),
                );
            }
        }
        let gains: Vec<[f32; 2]> = (0..=STEPS)
            .map(|step| gains_at(self.params, 2.0 * step as f32 / STEPS as f32))
            .collect();
        let trace = |channel: usize| {
            canvas::Path::new(|builder| {
                for (step, gain) in gains.iter().enumerate() {
                    let point = Point::new(
                        plot.x + plot.width * step as f32 / STEPS as f32,
                        y(gain[channel]),
                    );
                    if step == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            })
        };
        frame.stroke(
            &trace(1),
            canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &[5.0, 4.0],
                    offset: 0,
                },
                ..canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.5)
            },
        );
        frame.stroke(
            &trace(0),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        if labels && plot.width >= 160.0 {
            let key = Point::new(plot.x + plot.width - 70.0, plot.y + 8.0);
            // The key sits over the curves, so it gets its own ground.
            frame.fill(
                &canvas::Path::rounded_rectangle(
                    Point::new(key.x - 4.0, key.y - 7.0),
                    Size::new(70.0, 26.0),
                    2.0.into(),
                ),
                th::bg_dark(),
            );
            for (row, (name, dashed)) in [("L (Left)", false), ("R (Right)", true)]
                .into_iter()
                .enumerate()
            {
                let at = Point::new(key.x, key.y + row as f32 * 12.0);
                frame.stroke(
                    &canvas::Path::line(at, Point::new(at.x + 16.0, at.y)),
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: if dashed { &[4.0, 3.0] } else { &[] },
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(self.accent)
                            .with_width(1.5)
                    },
                );
                label(
                    &mut frame,
                    name.into(),
                    Point::new(at.x + 20.0, at.y),
                    iced::alignment::Horizontal::Left,
                    th::text_muted(),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// An arc from left to right with a dot where the output sits.
pub struct StereoPosition {
    /// -1 (left) to 1 (right), or `None` while silent.
    pub position: Option<f32>,
    pub accent: Color,
}

impl canvas::Program<Message> for StereoPosition {
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
        let radius = (bounds.width * 0.5 - 22.0)
            .min(bounds.height * 0.5 - 6.0)
            .max(4.0);
        let centre = Point::new(bounds.width * 0.5, bounds.height * 0.5 + radius * 0.15);
        let arc = canvas::Path::new(|builder| {
            builder.arc(canvas::path::Arc {
                center: centre,
                radius,
                start_angle: iced::Radians(std::f32::consts::PI * 0.75),
                end_angle: iced::Radians(std::f32::consts::PI * 2.25),
            });
        });
        frame.stroke(
            &arc,
            canvas::Stroke::default()
                .with_color(th::bg_hover())
                .with_width(5.0),
        );
        frame.stroke(
            &canvas::Path::line(
                Point::new(centre.x - radius, centre.y),
                Point::new(centre.x + radius, centre.y),
            ),
            canvas::Stroke::default()
                .with_color(th::text_muted())
                .with_width(1.0),
        );
        for (text, x, horizontal) in [
            (
                "L",
                centre.x - radius - 6.0,
                iced::alignment::Horizontal::Right,
            ),
            (
                "R",
                centre.x + radius + 6.0,
                iced::alignment::Horizontal::Left,
            ),
        ] {
            label(
                &mut frame,
                text.into(),
                Point::new(x, centre.y),
                horizontal,
                th::text(),
            );
        }
        let (position, color) = match self.position {
            Some(position) => (position.clamp(-1.0, 1.0), self.accent),
            None => (0.0, th::text_muted()),
        };
        frame.fill(
            &canvas::Path::circle(Point::new(centre.x + position * radius, centre.y), 4.5),
            color,
        );
        vec![frame.into_geometry()]
    }
}
