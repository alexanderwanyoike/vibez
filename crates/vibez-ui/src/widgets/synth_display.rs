//! The Subtractive Synth's plots, each drawn from the synth itself: an
//! oscillator's wave, the filter's response, an envelope's shape and the
//! LFO.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_instruments::synth::{lfo, svf_response_db, Waveform};
use vibez_instruments::synth_oscillator::sample;

use crate::message::Message;
use crate::spectrum::{freq_to_norm, norm_to_freq};
use crate::theme as th;

const LABEL_W: f32 = 26.0;
const LABEL_H: f32 = 14.0;
const DISPLAY_RATE: f32 = 48_000.0;
/// An envelope stage reaches -60 dB in its time, which the plot shows as
/// reaching the floor.
const DONE: f32 = 0.001;

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

/// Axis labels need this much height to sit apart.
const LABELS_MIN_H: f32 = 80.0;

/// The plot's area, with room for axis labels when asked for and tall
/// enough for them.
fn area(size: Size, axes: bool) -> (Rectangle, bool) {
    let labels = axes && size.height >= LABELS_MIN_H;
    let left = if labels { LABEL_W } else { 4.0 };
    let bottom = if labels { LABEL_H } else { 4.0 };
    (
        Rectangle::new(
            Point::new(left, 6.0),
            Size::new(
                (size.width - left - 8.0).max(1.0),
                (size.height - bottom - 10.0).max(1.0),
            ),
        ),
        labels,
    )
}

fn stroke(frame: &mut canvas::Frame, points: impl Iterator<Item = Point>, color: Color) {
    let path = canvas::Path::new(|builder| {
        for (index, point) in points.enumerate() {
            if index == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
    });
    frame.stroke(
        &path,
        canvas::Stroke::default().with_color(color).with_width(2.0),
    );
}

/// Two cycles of an oscillator's wave.
pub struct WavePlot {
    pub waveform: Waveform,
    pub accent: Color,
}

impl canvas::Program<Message> for WavePlot {
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
        let (plot, _) = area(bounds.size(), false);
        let y = |value: f32| plot.y + plot.height * 0.5 * (1.0 - value * 0.9);
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, y(0.0)),
                Point::new(plot.x + plot.width, y(0.0)),
            ),
            canvas::Stroke::default().with_color(grid()).with_width(1.0),
        );
        let steps = (plot.width as usize).max(8);
        stroke(
            &mut frame,
            (0..=steps).map(|step| {
                let position = step as f32 / steps as f32;
                // A slow enough note that the edge corrections do not show.
                let value = sample(self.waveform, f64::from(2.0 * position).fract(), 1e-4);
                Point::new(plot.x + position * plot.width, y(value))
            }),
            self.accent,
        );
        vec![frame.into_geometry()]
    }
}

/// A filter's gain from 20 Hz to 20 kHz, flat while it is off.
pub struct FilterPlot {
    /// One of the synth's filter types, or `None` when off.
    pub filter_type: Option<usize>,
    pub cutoff: f32,
    pub resonance: f32,
    pub accent: Color,
}

/// The top of a filter plot's axis: 0 dB, as in the concept, unless a
/// resonant peak rises above it, then the next 6 dB up.
fn axis_top_db(peak_db: f32) -> f32 {
    if peak_db <= 0.25 {
        0.0
    } else {
        ((peak_db / 6.0).ceil() * 6.0).min(24.0)
    }
}

impl canvas::Program<Message> for FilterPlot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        const FLOOR_DB: f32 = -48.0;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        panel(&mut frame, bounds.size());
        let (plot, labels) = area(bounds.size(), true);
        let steps = 160;
        let response: Vec<f32> = (0..=steps)
            .map(|step| {
                let position = step as f32 / steps as f32;
                self.filter_type.map_or(0.0, |kind| {
                    svf_response_db(
                        kind,
                        self.cutoff,
                        self.resonance,
                        norm_to_freq(position),
                        DISPLAY_RATE,
                    )
                })
            })
            .collect();
        let top_db = axis_top_db(response.iter().copied().fold(f32::MIN, f32::max));
        let y = |db: f32| {
            plot.y + plot.height * (top_db - db.clamp(FLOOR_DB, top_db)) / (top_db - FLOOR_DB)
        };
        let mut levels = vec![0.0f32, -12.0, -24.0, -36.0, -48.0];
        if top_db > 0.0 {
            levels.insert(0, top_db);
        }
        for db in levels {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
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
                    Point::new(LABEL_W - 4.0, y(db)),
                    iced::alignment::Horizontal::Right,
                );
            }
        }
        for (hz, text) in [
            (20.0, "20"),
            (100.0, "100"),
            (1_000.0, "1k"),
            (10_000.0, "10k"),
        ] {
            let x = plot.x + freq_to_norm(hz) * plot.width;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    text.into(),
                    Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    iced::alignment::Horizontal::Center,
                );
            }
        }
        let points: Vec<Point> = response
            .iter()
            .enumerate()
            .map(|(step, db)| Point::new(plot.x + plot.width * step as f32 / steps as f32, y(*db)))
            .collect();
        if self.filter_type.is_none() {
            // Off passes everything: a quiet flat line, named.
            stroke(&mut frame, points.into_iter(), th::text_muted());
            frame.fill_text(canvas::Text {
                content: "Off".into(),
                position: Point::new(plot.x + plot.width / 2.0, plot.y + plot.height * 0.55),
                color: th::text_muted(),
                size: 11.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }
        let area = canvas::Path::new(|builder| {
            builder.move_to(Point::new(plot.x, plot.y + plot.height));
            for point in &points {
                builder.line_to(*point);
            }
            builder.line_to(Point::new(plot.x + plot.width, plot.y + plot.height));
            builder.close();
        });
        frame.fill(
            &area,
            Color {
                a: 0.1,
                ..self.accent
            },
        );
        stroke(&mut frame, points.into_iter(), self.accent);
        vec![frame.into_geometry()]
    }
}

/// An ADSR's shape: a straight attack, then a decay and release that fall
/// to -60 dB in their times, with the sustain held between.
pub struct EnvelopePlot {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub accent: Color,
}

impl EnvelopePlot {
    /// Each stage's share of the width: its time, but never too thin to see.
    fn widths(&self) -> [f32; 4] {
        let total = self.attack + self.decay + self.release;
        let share = |seconds: f32| (seconds / total.max(1e-6)).max(0.06);
        let [a, d, r] = [share(self.attack), share(self.decay), share(self.release)];
        let hold = 0.25;
        let sum = a + d + r + hold;
        [a / sum, d / sum, hold / sum, r / sum]
    }
}

impl canvas::Program<Message> for EnvelopePlot {
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
        let (plot, _) = area(bounds.size(), false);
        let x = |position: f32| plot.x + plot.width * position;
        let y = |level: f32| plot.y + plot.height * (1.0 - level.clamp(0.0, 1.0));
        let [a, d, hold, r] = self.widths();
        for position in [a, a + d, a + d + hold] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(position), plot.y),
                    Point::new(x(position), plot.y + plot.height),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
        }
        let sustain = self.sustain.clamp(0.0, 1.0);
        let steps = 48;
        let falling = |from: f32, to: f32, t: f32| to + (from - to) * DONE.powf(t);
        let mut points = vec![Point::new(x(0.0), y(0.0)), Point::new(x(a), y(1.0))];
        points.extend((1..=steps).map(|step| {
            let t = step as f32 / steps as f32;
            Point::new(x(a + d * t), y(falling(1.0, sustain, t)))
        }));
        points.push(Point::new(x(a + d + hold), y(sustain)));
        points.extend((1..=steps).map(|step| {
            let t = step as f32 / steps as f32;
            Point::new(x(a + d + hold + r * t), y(falling(sustain, 0.0, t)))
        }));
        stroke(&mut frame, points.into_iter(), self.accent);
        for (position, level) in [(a, 1.0), (a + d, sustain), (a + d + hold, sustain)] {
            frame.fill(
                &canvas::Path::circle(Point::new(x(position), y(level)), 3.5),
                self.accent,
            );
        }
        vec![frame.into_geometry()]
    }
}

/// Two cycles of the LFO, -1 to 1.
pub struct LfoPlot {
    pub shape: usize,
    pub accent: Color,
}

impl canvas::Program<Message> for LfoPlot {
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
        let (plot, _) = area(bounds.size(), false);
        let y = |value: f32| plot.y + plot.height * 0.5 * (1.0 - value * 0.9);
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, y(0.0)),
                Point::new(plot.x + plot.width, y(0.0)),
            ),
            canvas::Stroke::default().with_color(grid()).with_width(1.0),
        );
        let steps = (plot.width as usize).max(8);
        stroke(
            &mut frame,
            (0..=steps).map(|step| {
                let position = step as f32 / steps as f32;
                let value = lfo(self.shape, f64::from(2.0 * position));
                Point::new(plot.x + position * plot.width, y(value))
            }),
            self.accent,
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_axis_tops_out_at_zero_unless_a_peak_rises_above() {
        assert_eq!(axis_top_db(-3.0), 0.0);
        assert_eq!(axis_top_db(0.1), 0.0, "a Butterworth corner is flat");
        assert_eq!(axis_top_db(1.3), 6.0);
        assert_eq!(axis_top_db(13.0), 18.0);
        assert_eq!(axis_top_db(40.0), 24.0);
    }

    #[test]
    fn every_envelope_stage_stays_visible_and_the_widths_fill_the_plot() {
        let plot = EnvelopePlot {
            attack: 0.001,
            decay: 2.0,
            sustain: 0.5,
            release: 5.0,
            accent: Color::WHITE,
        };
        let widths = plot.widths();
        assert!((widths.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(widths.iter().all(|width| *width > 0.03), "{widths:?}");
        assert!(widths[3] > widths[1], "the longer release is wider");
    }
}
