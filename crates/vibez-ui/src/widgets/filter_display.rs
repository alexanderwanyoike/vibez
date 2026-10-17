//! The Filter's plots: its static response from the DSP with the cutoff as a
//! node to drag, and the shape of its cutoff modulation.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::{EffectId, TrackId};
use vibez_dsp::filter::{param, response_magnitude, CUTOFF_MAX, CUTOFF_MIN};

use crate::message::Message;
use crate::spectrum::{freq_to_norm, norm_to_freq};
use crate::state::UndoGestureId;
use crate::theme as th;

const TOP_DB: f32 = 12.0;
const FLOOR_DB: f32 = -48.0;
const LABEL_W: f32 = 24.0;
const LABEL_H: f32 = 14.0;
const NODE_R: f32 = 5.0;

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

fn grid() -> Color {
    Color {
        a: 0.35,
        ..th::border()
    }
}

/// Gain against frequency, with the cutoff as a node to drag.
pub struct ResponsePlot<'a> {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub params: &'a [f32],
    pub sample_rate: f32,
    pub accent: Color,
}

#[derive(Debug, Default)]
pub struct ResponseState {
    gesture: Option<UndoGestureId>,
}

impl ResponsePlot<'_> {
    fn plot(size: Size) -> Rectangle {
        let labels = size.height >= 60.0;
        let left = if labels { LABEL_W } else { 4.0 };
        let bottom = if labels { LABEL_H } else { 4.0 };
        Rectangle::new(
            Point::new(left, 4.0),
            Size::new(
                (size.width - left - 6.0).max(1.0),
                (size.height - bottom - 6.0).max(1.0),
            ),
        )
    }

    fn value(&self, index: usize) -> f32 {
        self.params
            .get(index)
            .copied()
            .unwrap_or(vibez_dsp::filter::FILTER_PARAMS[index].default)
    }

    fn y(plot: Rectangle, db: f32) -> f32 {
        plot.y + (TOP_DB - db.clamp(FLOOR_DB, TOP_DB)) / (TOP_DB - FLOOR_DB) * plot.height
    }

    fn db_at(&self, frequency: f32) -> f32 {
        let magnitude = response_magnitude(
            self.sample_rate,
            self.value(param::CUTOFF),
            self.value(param::RESONANCE),
            self.value(param::MODE),
            self.value(param::SLOPE),
            self.value(param::MIX),
            frequency,
        );
        vibez_core::level::amplitude_to_db(magnitude.max(1e-6))
    }

    fn node(&self, plot: Rectangle) -> Point {
        let cutoff = self.value(param::CUTOFF);
        Point::new(
            plot.x + freq_to_norm(cutoff) * plot.width,
            Self::y(plot, self.db_at(cutoff)),
        )
    }

    fn cutoff_at(plot: Rectangle, x: f32) -> f32 {
        norm_to_freq(((x - plot.x) / plot.width).clamp(0.0, 1.0)).clamp(CUTOFF_MIN, CUTOFF_MAX)
    }

    fn near_node(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        let plot = Self::plot(bounds.size());
        cursor
            .position_in(bounds)
            .is_some_and(|at| at.distance(self.node(plot)) <= NODE_R + 4.0)
    }
}

impl canvas::Program<Message> for ResponsePlot<'_> {
    type State = ResponseState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill(
            &canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 3.0.into()),
            th::bg_dark(),
        );
        let plot = Self::plot(bounds.size());
        let labels = bounds.height >= 60.0;
        for db in [12.0f32, 0.0, -12.0, -24.0, -36.0, -48.0] {
            let y = Self::y(plot, db);
            frame.stroke(
                &canvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y),
                    iced::alignment::Horizontal::Right,
                );
            }
        }
        // Narrow plots drop the labels that would overlap the one before.
        let mut labelled_to = f32::NEG_INFINITY;
        for (frequency, text) in [
            (20.0, "20"),
            (100.0, "100"),
            (1_000.0, "1k"),
            (10_000.0, "10k"),
            (20_000.0, "20k"),
        ] {
            let x = plot.x + freq_to_norm(frequency) * plot.width;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            let text_w = text.len() as f32 * 5.5;
            let (left, horizontal) = if frequency >= 20_000.0 {
                (x - text_w, iced::alignment::Horizontal::Right)
            } else {
                (x - text_w / 2.0, iced::alignment::Horizontal::Center)
            };
            if labels && left >= labelled_to + 4.0 {
                labelled_to = left + text_w;
                label(
                    &mut frame,
                    text.into(),
                    Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    horizontal,
                );
            }
        }
        let points: Vec<Point> = (0..=128)
            .map(|step| {
                let position = step as f32 / 128.0;
                Point::new(
                    plot.x + position * plot.width,
                    Self::y(plot, self.db_at(norm_to_freq(position))),
                )
            })
            .collect();
        let fill = canvas::Path::new(|builder| {
            builder.move_to(Point::new(plot.x, plot.y + plot.height));
            for point in &points {
                builder.line_to(*point);
            }
            builder.line_to(Point::new(plot.x + plot.width, plot.y + plot.height));
            builder.close();
        });
        frame.fill(
            &fill,
            Color {
                a: 0.16,
                ..self.accent
            },
        );
        let curve = canvas::Path::new(|builder| {
            builder.move_to(points[0]);
            for point in &points[1..] {
                builder.line_to(*point);
            }
        });
        frame.stroke(
            &curve,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        let node = self.node(plot);
        let engaged = state.gesture.is_some() || self.near_node(bounds, cursor);
        frame.fill(
            &canvas::Path::circle(node, if engaged { NODE_R + 1.0 } else { NODE_R }),
            self.accent,
        );
        frame.fill_text(canvas::Text {
            content: crate::domains::param_entry::ParamFormat::Hertz
                .format(self.value(param::CUTOFF)),
            position: Point::new(
                node.x.clamp(plot.x + 28.0, plot.x + plot.width - 28.0),
                (node.y - 12.0).max(plot.y + 6.0),
            ),
            color: th::text(),
            size: 10.0.into(),
            horizontal_alignment: iced::alignment::Horizontal::Center,
            vertical_alignment: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if self.near_node(bounds, cursor) =>
            {
                state.gesture = Some(UndoGestureId::new());
                return (canvas::event::Status::Captured, None);
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.gesture.take().is_some() =>
            {
                return (canvas::event::Status::Captured, None);
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(gesture), Some(at)) = (state.gesture, cursor.position()) {
                    let plot = Self::plot(bounds.size());
                    let cutoff = Self::cutoff_at(plot, at.x - bounds.x);
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_effect_param(
                                self.track_id,
                                self.effect_id,
                                param::CUTOFF,
                                cutoff,
                            )
                            .in_undo_gesture(gesture),
                        ),
                    );
                }
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.gesture.is_some() {
            mouse::Interaction::Grabbing
        } else if self.near_node(bounds, cursor) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// The shape of the cutoff modulation: two cycles of the LFO's sine, its
/// right channel offset by the stereo phase, or the envelope's rise and
/// fall. The Amount readout carries the depth, so the shape fills the box
/// and only dims when there is none.
pub struct ModulationPreview<'a> {
    pub params: &'a [f32],
    pub accent: Color,
}

impl canvas::Program<Message> for ModulationPreview<'_> {
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
        let value = |index: usize| {
            self.params
                .get(index)
                .copied()
                .unwrap_or(vibez_dsp::filter::FILTER_PARAMS[index].default)
        };
        let source = value(param::MOD_SOURCE).round();
        let color = if value(param::DEPTH) > 0.0 {
            self.accent
        } else {
            th::text_muted()
        };
        let (w, h) = (bounds.width, bounds.height);
        let middle = h / 2.0;
        let swing = h / 2.0 - 4.0;
        let curve = |offset: f32| {
            canvas::Path::new(|builder| {
                for step in 0..=64 {
                    let position = step as f32 / 64.0;
                    let shape = if source == 2.0 {
                        let attack = value(param::ENV_ATTACK);
                        let release = value(param::ENV_RELEASE);
                        let rise = attack / (attack + release);
                        if position < rise {
                            position / rise.max(1e-3)
                        } else {
                            (-(position - rise) / (1.0 - rise).max(1e-3) * 3.0).exp()
                        }
                    } else {
                        (std::f32::consts::TAU * (2.0 * position + offset)).sin()
                    };
                    let point = Point::new(position * w, middle - shape * swing);
                    if step == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            })
        };
        if source == 1.0 && value(param::STEREO_PHASE) > 0.0 {
            frame.stroke(
                &curve(value(param::STEREO_PHASE) / 360.0),
                canvas::Stroke::default()
                    .with_color(Color { a: 0.45, ..color })
                    .with_width(1.5),
            );
        }
        if source > 0.0 {
            frame.stroke(
                &curve(0.0),
                canvas::Stroke::default().with_color(color).with_width(1.5),
            );
        } else {
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, middle), Point::new(w, middle)),
                canvas::Stroke::default()
                    .with_color(th::text_muted())
                    .with_width(1.0),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_maps_the_plot_width_to_the_cutoff_range_and_clamps() {
        let plot = ResponsePlot::plot(Size::new(424.0, 200.0));
        assert!((ResponsePlot::cutoff_at(plot, plot.x) - CUTOFF_MIN).abs() < 1e-3);
        assert!((ResponsePlot::cutoff_at(plot, plot.x + plot.width) - CUTOFF_MAX).abs() < 1.0);
        let beyond = ResponsePlot::cutoff_at(plot, plot.x + plot.width * 3.0);
        assert!(beyond <= CUTOFF_MAX && beyond > CUTOFF_MAX - 1.0);
    }

    #[test]
    fn the_cutoff_node_sits_on_the_response_curve() {
        let params: Vec<f32> = vibez_dsp::filter::FILTER_PARAMS
            .iter()
            .map(|descriptor| descriptor.default)
            .collect();
        let response = ResponsePlot {
            track_id: TrackId::new(),
            effect_id: EffectId::new(),
            params: &params,
            sample_rate: 48_000.0,
            accent: Color::WHITE,
        };
        let plot = ResponsePlot::plot(Size::new(424.0, 200.0));
        let node = response.node(plot);
        let cutoff = params[param::CUTOFF];
        assert!((node.y - ResponsePlot::y(plot, response.db_at(cutoff))).abs() < 1e-3);
        assert!(
            response.db_at(cutoff) < -2.0 && response.db_at(cutoff) > -4.0,
            "a 0.707 Q low-pass is 3 dB down at its cutoff"
        );
    }
}
