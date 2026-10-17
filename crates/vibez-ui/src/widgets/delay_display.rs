//! The Delay's echoes over the next beats: when each repeat arrives and how
//! loud, from its times and feedback. Ping-pong shows a sound that starts
//! on the left bouncing between the sides.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::delay::{line_times_ms, param, DELAY_PARAMS};

use crate::message::Message;
use crate::theme as th;

const FLOOR_DB: f32 = -48.0;
const LABEL_W: f32 = 30.0;
const LABEL_H: f32 = 14.0;
const MIN_BEATS: f64 = 4.0;
const MAX_TAPS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tap {
    pub beats: f64,
    pub db: f32,
    pub right: bool,
}

/// The repeats a single sound makes, in order, until they fall below the
/// floor.
pub fn taps(params: &[f32], bpm: f64) -> Vec<Tap> {
    let value = |index: usize| {
        params
            .get(index)
            .copied()
            .unwrap_or(DELAY_PARAMS[index].default)
    };
    let beats_per_ms = bpm.max(1.0) / 60_000.0;
    let [left, right] = line_times_ms(params, bpm).map(|ms| ms * beats_per_ms);
    let feedback = value(param::FEEDBACK).clamp(0.0, 0.95);
    let ping_pong = value(param::ROUTING) >= 0.5;
    let db = |repeat: i32| {
        if repeat == 0 {
            0.0
        } else if feedback <= 0.0 {
            f32::NEG_INFINITY
        } else {
            20.0 * repeat as f32 * feedback.log10()
        }
    };
    let mut taps = Vec::new();
    if ping_pong {
        let mut at = 0.0;
        for repeat in 0..MAX_TAPS as i32 {
            let on_right = repeat % 2 == 1;
            at += if on_right { right } else { left };
            if db(repeat) < FLOOR_DB {
                break;
            }
            taps.push(Tap {
                beats: at,
                db: db(repeat),
                right: on_right,
            });
        }
    } else {
        let linked = value(param::LINK) >= 0.5;
        for (time, on_right) in [(left, false), (right, true)] {
            if on_right && linked {
                break;
            }
            for repeat in 0..MAX_TAPS as i32 / 2 {
                if db(repeat) < FLOOR_DB {
                    break;
                }
                taps.push(Tap {
                    beats: time * f64::from(repeat + 1),
                    db: db(repeat),
                    right: on_right,
                });
            }
        }
    }
    taps
}

pub struct EchoPlot<'a> {
    pub params: &'a [f32],
    pub bpm: f64,
    pub accent: Color,
}

impl canvas::Program<Message> for EchoPlot<'_> {
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
        let bottom = if labels { LABEL_H } else { 4.0 };
        let plot = Rectangle::new(
            Point::new(left, 6.0),
            Size::new(
                (bounds.width - left - 8.0).max(1.0),
                (bounds.height - bottom - 8.0).max(1.0),
            ),
        );
        let taps = taps(self.params, self.bpm);
        let first = taps.first().map_or(1.0, |tap| tap.beats);
        let span = MIN_BEATS.max((first * 4.0).ceil());
        let x = |beats: f64| plot.x + plot.width * (beats / span).clamp(0.0, 1.0) as f32;
        let y = |db: f32| plot.y + plot.height * (db.clamp(FLOOR_DB, 0.0) / FLOOR_DB);
        let grid = Color {
            a: 0.35,
            ..th::border()
        };
        let label = |frame: &mut canvas::Frame, text: String, at: Point, right: bool| {
            frame.fill_text(canvas::Text {
                content: text,
                position: at,
                color: th::text_muted(),
                size: 9.0.into(),
                horizontal_alignment: if right {
                    iced::alignment::Horizontal::Right
                } else {
                    iced::alignment::Horizontal::Center
                },
                vertical_alignment: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        };
        for db in [0.0f32, -12.0, -24.0, -36.0, -48.0] {
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
                label(&mut frame, text, Point::new(LABEL_W - 4.0, y(db)), true);
            }
        }
        let step = (span / 4.0).ceil().max(1.0);
        let mut beat = 0.0;
        while beat <= span {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(beat), plot.y),
                    Point::new(x(beat), plot.y + plot.height),
                ),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    format!("{beat:.0}"),
                    Point::new(x(beat), plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    false,
                );
            }
            beat += step;
        }
        let stem = |frame: &mut canvas::Frame, beats: f64, db: f32, color: Color| {
            let top = Point::new(x(beats), y(db));
            frame.stroke(
                &canvas::Path::line(Point::new(top.x, plot.y + plot.height), top),
                canvas::Stroke::default().with_color(color).with_width(2.0),
            );
            frame.fill(&canvas::Path::circle(top, 4.0), color);
        };
        stem(&mut frame, 0.0, 0.0, th::text_dim());
        let right_color = Color {
            a: 0.55,
            ..self.accent
        };
        for tap in taps.iter().filter(|tap| tap.beats <= span) {
            stem(
                &mut frame,
                tap.beats,
                tap.db,
                if tap.right { right_color } else { self.accent },
            );
        }
        if labels {
            label(
                &mut frame,
                "Beats".into(),
                Point::new(plot.x + plot.width, plot.y + plot.height - 8.0),
                true,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(edits: &[(usize, f32)]) -> Vec<f32> {
        let mut params: Vec<f32> = DELAY_PARAMS.iter().map(|d| d.default).collect();
        for &(index, value) in edits {
            params[index] = value;
        }
        params
    }

    #[test]
    fn linked_stereo_repeats_fall_by_the_feedback_each_time() {
        let taps = taps(&params(&[(param::FEEDBACK, 0.5)]), 120.0);
        assert_eq!(taps[0].beats, 1.0);
        assert_eq!(taps[0].db, 0.0);
        assert_eq!(taps[1].beats, 2.0);
        assert!((taps[1].db + 6.02).abs() < 0.01);
        assert!(taps.iter().all(|tap| !tap.right && tap.db >= FLOOR_DB));
    }

    #[test]
    fn ping_pong_alternates_sides_at_each_lines_time() {
        let taps = taps(
            &params(&[
                (param::ROUTING, 1.0),
                (param::LINK, 0.0),
                (param::RIGHT_TIME_MS, 250.0),
            ]),
            120.0,
        );
        assert_eq!(
            taps[..3]
                .iter()
                .map(|tap| (tap.beats, tap.right))
                .collect::<Vec<_>>(),
            vec![(1.0, false), (1.5, true), (2.5, false)]
        );
    }

    #[test]
    fn no_feedback_leaves_a_single_echo() {
        assert_eq!(taps(&params(&[(param::FEEDBACK, 0.0)]), 120.0).len(), 1);
    }
}
