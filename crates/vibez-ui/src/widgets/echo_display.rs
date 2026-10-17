//! The Echo's plots: its repeats over the next bars, each smeared as wear
//! and diffusion leave it, and how far wow and flutter pull the pitch.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_dsp::echo::{
    echo_taps_db, echo_time_ms, effective, head_travel_ms, mode, param, ECHO_PARAMS,
};

use crate::message::Message;
use crate::theme as th;

const FLOOR_DB: f32 = -48.0;
const LABEL_W: f32 = 30.0;
const LABEL_H: f32 = 14.0;
const MAX_TAPS: usize = 48;
/// Bars a plot may span; it shows enough repeats to read their fall.
const MIN_BARS: f64 = 1.0;
const MAX_BARS: f64 = 4.0;
/// Seconds of head motion the pitch trace shows.
const MOTION_SECONDS: f64 = 10.0;

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(ECHO_PARAMS[index].default)
}

/// A bar position as the axis spells it: "3/4", "1", "1 1/4".
pub fn bar_label(quarters: u32) -> String {
    let (whole, part) = (quarters / 4, quarters % 4);
    let fraction = ["", "1/4", "1/2", "3/4"][part as usize];
    match (whole, part) {
        (0, 0) => "0".into(),
        (0, _) => fraction.into(),
        (_, 0) => whole.to_string(),
        _ => format!("{whole} {fraction}"),
    }
}

/// The bars the plot spans for repeats `time_bars` apart: about six of
/// them, in whole quarter bars.
pub fn span_bars(time_bars: f64) -> f64 {
    ((time_bars * 6.0 * 4.0).ceil() / 4.0).clamp(MIN_BARS, MAX_BARS)
}

/// The pitch wow and flutter give the repeats `seconds` in, in cents.
pub fn pitch_cents(params: &[f32], seconds: f64) -> f64 {
    let step = 0.001;
    let slope = (head_travel_ms(params, seconds + step) - head_travel_ms(params, seconds - step))
        / (2.0 * step * 1000.0);
    // The head moving away by `slope` seconds a second plays slower by as
    // much.
    1200.0 * (1.0 - slope).max(0.01).log2()
}

pub struct TapsPlot<'a> {
    pub params: &'a [f32],
    pub bpm: f64,
    pub accent: Color,
}

impl canvas::Program<Message> for TapsPlot<'_> {
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
                (bounds.width - left - 10.0).max(1.0),
                (bounds.height - bottom - 8.0).max(1.0),
            ),
        );
        let bar_ms = 240_000.0 / self.bpm.max(1.0);
        let time_bars = echo_time_ms(self.params, self.bpm) / bar_ms;
        let span = span_bars(time_bars);
        let x = |bars: f64| plot.x + plot.width * (bars / span).clamp(0.0, 1.0) as f32;
        let y = |db: f32| plot.y + plot.height * (db.clamp(FLOOR_DB, 0.0) / FLOOR_DB);
        let grid = th::with_alpha(th::border(), 0.35);
        let label = |frame: &mut canvas::Frame, text: String, at: Point, right: bool| {
            frame.fill_text(canvas::Text {
                content: text,
                position: at,
                color: th::text_muted(),
                size: crate::widgets::device_controls::LABEL_SIZE.into(),
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
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y(db)),
                    true,
                );
            }
        }
        let quarters = (span * 4.0).round() as u32;
        // Every quarter bar on a short span, every half on a long one.
        let every = if quarters > 8 { 2 } else { 1 };
        for quarter in (0..=quarters).step_by(every) {
            let at = x(f64::from(quarter) / 4.0);
            frame.stroke(
                &canvas::Path::line(Point::new(at, plot.y), Point::new(at, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    bar_label(quarter),
                    Point::new(at, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    false,
                );
            }
        }

        let taps = echo_taps_db(self.params, MAX_TAPS);
        let ping_pong = value(self.params, param::MODE).round() as usize == mode::PING_PONG;
        let smear = (effective(self.params, param::DIFFUSION)
            + 0.5 * effective(self.params, param::WEAR))
        .min(1.5);
        let width = (plot.width / 80.0).clamp(4.0, 10.0);
        let mut tops = Vec::new();
        for (repeat, db) in taps.iter().enumerate() {
            let at = time_bars * (repeat + 1) as f64;
            if at > span || *db < FLOOR_DB {
                break;
            }
            let color = if ping_pong && repeat % 2 == 1 {
                th::with_alpha(self.accent, 0.55)
            } else {
                self.accent
            };
            let top = Point::new(x(at), y(*db));
            // The tail each repeat drags, as wear and diffusion spread it:
            // a soft block behind, then a few fainter echoes of the bar.
            if smear > 0.0 {
                let tail = width * (1.5 + 4.0 * smear);
                frame.fill_rectangle(
                    Point::new(top.x - width / 2.0, top.y + 2.0),
                    Size::new(tail, (plot.y + plot.height - top.y - 2.0).max(0.0)),
                    th::with_alpha(color, 0.12 + 0.12 * smear.min(1.0)),
                );
                for trail in 1..=3 {
                    let offset = trail as f32 * tail / 3.0;
                    let drop = 4.0 * trail as f32;
                    frame.fill_rectangle(
                        Point::new(top.x + offset - width / 4.0, top.y + drop),
                        Size::new(width / 2.0, (plot.y + plot.height - top.y - drop).max(0.0)),
                        th::with_alpha(color, 0.35 * (1.0 - trail as f32 / 4.0)),
                    );
                }
            }
            frame.fill_rectangle(
                Point::new(top.x - width / 2.0, top.y),
                Size::new(width, plot.y + plot.height - top.y),
                color,
            );
            tops.push(top);
        }
        if tops.len() > 1 {
            let mut curve = canvas::path::Builder::new();
            curve.move_to(tops[0]);
            for top in &tops[1..] {
                curve.line_to(*top);
            }
            let mut stroke = canvas::Stroke::default()
                .with_color(th::with_alpha(self.accent, 0.8))
                .with_width(1.0);
            stroke.line_dash = canvas::LineDash {
                segments: &[2.0, 3.0],
                offset: 0,
            };
            frame.stroke(&curve.build(), stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// How far wow and flutter move the play head over the next seconds, in ms.
pub struct MotionPlot<'a> {
    pub params: &'a [f32],
    pub accent: Color,
}

impl canvas::Program<Message> for MotionPlot<'_> {
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
        let left = 26.0;
        let plot = Rectangle::new(
            Point::new(left, 4.0),
            Size::new(
                (bounds.width - left - 4.0).max(1.0),
                (bounds.height - 8.0).max(1.0),
            ),
        );
        let columns = plot.width.max(1.0) as usize;
        // How far the head wanders, which wow rules and flutter ripples.
        let travel: Vec<f64> = (0..=columns)
            .map(|column| {
                head_travel_ms(self.params, MOTION_SECONDS * column as f64 / columns as f64)
            })
            .collect();
        let reach = (travel.iter().fold(0.0f64, |reach, ms| reach.max(ms.abs())) * 1.25).max(0.05);
        let y = |cent: f64| plot.y + plot.height * (0.5 - (cent / reach) as f32 * 0.5);
        for (cent, text) in [
            (reach, format!("+{reach:.1}")),
            (0.0, "0".into()),
            (-reach, format!("-{reach:.1}")),
        ] {
            frame.fill_text(canvas::Text {
                content: text,
                position: Point::new(left - 4.0, y(cent)),
                color: th::text_muted(),
                size: crate::widgets::device_controls::LABEL_SIZE.into(),
                horizontal_alignment: iced::alignment::Horizontal::Right,
                vertical_alignment: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, y(0.0)),
                Point::new(plot.x + plot.width, y(0.0)),
            ),
            canvas::Stroke::default()
                .with_color(th::with_alpha(th::border(), 0.5))
                .with_width(1.0),
        );
        let mut line = canvas::path::Builder::new();
        for (column, cent) in travel.iter().enumerate() {
            let point = Point::new(plot.x + column as f32, y(*cent));
            if column == 0 {
                line.move_to(point);
            } else {
                line.line_to(point);
            }
        }
        frame.stroke(
            &line.build(),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(1.5),
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::echo::style;

    fn params(edits: &[(usize, f32)]) -> Vec<f32> {
        let mut params: Vec<f32> = ECHO_PARAMS.iter().map(|d| d.default).collect();
        for &(index, value) in edits {
            params[index] = value;
        }
        params
    }

    #[test]
    fn the_axis_counts_in_quarter_bars() {
        assert_eq!(bar_label(0), "0");
        assert_eq!(bar_label(1), "1/4");
        assert_eq!(bar_label(4), "1");
        assert_eq!(bar_label(5), "1 1/4");
        assert_eq!(bar_label(7), "1 3/4");
    }

    #[test]
    fn the_span_shows_about_six_repeats_within_its_limits() {
        // A dotted quarter is 3/8 of a bar.
        assert_eq!(span_bars(0.375), 2.25);
        assert_eq!(span_bars(0.01), MIN_BARS);
        assert_eq!(span_bars(2.0), MAX_BARS);
    }

    #[test]
    fn clean_repeats_hold_their_pitch_and_wow_pulls_it() {
        let clean = params(&[(param::STYLE, style::CLEAN as f32)]);
        assert!((0..80).all(|i| pitch_cents(&clean, f64::from(i) / 10.0).abs() < 1e-6));
        let wobbly = params(&[(param::CHARACTER, 1.0), (param::WOW, 1.0)]);
        let swing = (0..80)
            .map(|i| pitch_cents(&wobbly, f64::from(i) / 10.0).abs())
            .fold(0.0f64, f64::max);
        assert!(swing > 5.0 && swing < 40.0, "{swing}");
    }
}
