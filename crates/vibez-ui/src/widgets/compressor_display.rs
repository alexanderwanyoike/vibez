//! The Compressor's plots: its static transfer curve from the DSP, the
//! measured gain reduction over time, and the reduction meter.

use std::collections::VecDeque;
use std::time::Instant;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::{EffectId, TrackId};
use vibez_dsp::compressor::{param, static_transfer_db};

use crate::domains::compressor::{ReductionPoint, HISTORY};
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme as th;

/// Both transfer axes run from here to 0 dBFS.
pub const TRANSFER_FLOOR_DB: f32 = -48.0;
pub const HISTORY_FLOOR_DB: f32 = 24.0;
pub const METER_FLOOR_DB: f32 = 12.0;
const LABEL_W: f32 = 24.0;
const LABEL_H: f32 = 14.0;
const NODE_R: f32 = 5.0;

fn panel(frame: &mut canvas::Frame, size: Size) {
    frame.fill(
        &canvas::Path::rounded_rectangle(Point::ORIGIN, size, 3.0.into()),
        th::bg_dark(),
    );
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

/// Input level against output level, with the threshold as a node to drag.
pub struct TransferPlot<'a> {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub params: &'a [f32],
    pub accent: Color,
    pub threshold_range: (f32, f32),
}

#[derive(Debug, Default)]
pub struct TransferState {
    gesture: Option<UndoGestureId>,
}

impl TransferPlot<'_> {
    fn plot(bounds: Size) -> Rectangle {
        Rectangle::new(
            Point::new(LABEL_W, 4.0),
            Size::new(
                (bounds.width - LABEL_W - 6.0).max(1.0),
                (bounds.height - LABEL_H - 6.0).max(1.0),
            ),
        )
    }

    fn x(plot: Rectangle, db: f32) -> f32 {
        plot.x + (db - TRANSFER_FLOOR_DB) / -TRANSFER_FLOOR_DB * plot.width
    }

    fn y(plot: Rectangle, db: f32) -> f32 {
        let db = db.clamp(TRANSFER_FLOOR_DB, 0.0);
        plot.y + db / TRANSFER_FLOOR_DB * plot.height
    }

    fn threshold(&self) -> f32 {
        self.params.get(param::THRESHOLD).copied().unwrap_or(-18.0)
    }

    fn node(&self, plot: Rectangle) -> Point {
        let threshold = self.threshold();
        Point::new(
            Self::x(plot, threshold),
            Self::y(plot, static_transfer_db(threshold, self.params)),
        )
    }

    fn threshold_at(&self, plot: Rectangle, x: f32) -> f32 {
        let db = TRANSFER_FLOOR_DB + (x - plot.x) / plot.width * -TRANSFER_FLOOR_DB;
        let (min, max) = self.threshold_range;
        (db.clamp(min, max) * 10.0).round() / 10.0
    }
}

impl canvas::Program<Message> for TransferPlot<'_> {
    type State = TransferState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        panel(&mut frame, bounds.size());
        let plot = Self::plot(bounds.size());
        for db in [-48.0f32, -36.0, -24.0, -12.0, 0.0] {
            let (x, y) = (Self::x(plot, db), Self::y(plot, db));
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            frame.stroke(
                &canvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            label(
                &mut frame,
                format!("{db:.0}"),
                Point::new(LABEL_W - 4.0, y),
                iced::alignment::Horizontal::Right,
            );
            label(
                &mut frame,
                format!("{db:.0}"),
                Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                iced::alignment::Horizontal::Center,
            );
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(
                    Self::x(plot, TRANSFER_FLOOR_DB),
                    Self::y(plot, TRANSFER_FLOOR_DB),
                ),
                Point::new(Self::x(plot, 0.0), Self::y(plot, 0.0)),
            ),
            canvas::Stroke::default()
                .with_color(th::text_muted())
                .with_width(1.0),
        );
        let curve = canvas::Path::new(|builder| {
            for step in 0..=96 {
                let input = TRANSFER_FLOOR_DB + step as f32 / 96.0 * -TRANSFER_FLOOR_DB;
                let point = Point::new(
                    Self::x(plot, input),
                    Self::y(plot, static_transfer_db(input, self.params)),
                );
                if step == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &curve,
            canvas::Stroke {
                line_join: canvas::LineJoin::Round,
                ..canvas::Stroke::default()
            }
            .with_color(self.accent)
            .with_width(2.0),
        );
        let node = self.node(plot);
        let engaged = state.gesture.is_some()
            || cursor
                .position_in(bounds)
                .is_some_and(|at| at.distance(node) <= NODE_R + 4.0);
        frame.fill(
            &canvas::Path::circle(node, if engaged { NODE_R + 1.0 } else { NODE_R }),
            self.accent,
        );
        frame.fill_text(canvas::Text {
            content: format!("{:.1} dB", self.threshold()),
            position: Point::new(
                node.x.clamp(plot.x + 24.0, plot.x + plot.width - 24.0),
                node.y - 12.0,
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
        let plot = Self::plot(bounds.size());
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor
                    .position_in(bounds)
                    .is_some_and(|at| at.distance(self.node(plot)) <= NODE_R + 4.0)
                {
                    state.gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.gesture.take().is_some() =>
            {
                return (canvas::event::Status::Captured, None);
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(gesture), Some(at)) = (state.gesture, cursor.position()) {
                    let threshold = self.threshold_at(plot, at.x - bounds.x);
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_effect_param(
                                self.track_id,
                                self.effect_id,
                                param::THRESHOLD,
                                threshold,
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
        let plot = Self::plot(bounds.size());
        if state.gesture.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor
            .position_in(bounds)
            .is_some_and(|at| at.distance(self.node(plot)) <= NODE_R + 4.0)
        {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Gain reduction over the last few seconds, deepest at the bottom.
pub struct ReductionHistory<'a> {
    pub points: &'a VecDeque<ReductionPoint>,
    pub now: Instant,
    pub accent: Color,
}

impl canvas::Program<Message> for ReductionHistory<'_> {
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
        let plot = Rectangle::new(
            Point::new(LABEL_W, 4.0),
            Size::new(
                (bounds.width - LABEL_W - 6.0).max(1.0),
                (bounds.height - 8.0).max(1.0),
            ),
        );
        let y = |db: f32| plot.y + db.clamp(0.0, HISTORY_FLOOR_DB) / HISTORY_FLOOR_DB * plot.height;
        for db in [0.0f32, 6.0, 12.0, 18.0, 24.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            label(
                &mut frame,
                if db == 0.0 {
                    "0".into()
                } else {
                    format!("-{db:.0}")
                },
                Point::new(LABEL_W - 4.0, y(db)),
                iced::alignment::Horizontal::Right,
            );
        }
        let span = HISTORY.as_secs_f32();
        let x = |at: Instant| {
            let age = self.now.saturating_duration_since(at).as_secs_f32();
            plot.x + plot.width * (1.0 - age / span).clamp(0.0, 1.0)
        };
        if self.points.len() > 1 {
            let line = canvas::Path::new(|builder| {
                for (index, point) in self.points.iter().enumerate() {
                    let at = Point::new(x(point.at), y(point.db));
                    if index == 0 {
                        builder.move_to(at);
                    } else {
                        builder.line_to(at);
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
        vec![frame.into_geometry()]
    }
}

/// How deep the compressor is cutting now, filling down from 0 dB.
pub struct ReductionMeter {
    pub db: f32,
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
        let bar = Rectangle::new(
            Point::new(0.0, 4.0),
            Size::new(10.0, (bounds.height - 8.0).max(1.0)),
        );
        frame.fill_rectangle(bar.position(), bar.size(), th::bg_dark());
        let depth = (self.db / METER_FLOOR_DB).clamp(0.0, 1.0) * bar.height;
        frame.fill_rectangle(bar.position(), Size::new(bar.width, depth), self.accent);
        for db in [0.0f32, 3.0, 6.0, 9.0, 12.0] {
            label(
                &mut frame,
                if db == 0.0 {
                    "0".into()
                } else {
                    format!("-{db:.0}")
                },
                Point::new(bar.width + 4.0, bar.y + db / METER_FLOOR_DB * bar.height),
                iced::alignment::Horizontal::Left,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> TransferPlot<'static> {
        static PARAMS: [f32; param::COUNT] = {
            let mut params = [0.0; param::COUNT];
            params[param::THRESHOLD] = -18.0;
            params[param::RATIO] = 4.0;
            params[param::MIX] = 1.0;
            params
        };
        TransferPlot {
            track_id: TrackId::new(),
            effect_id: EffectId::new(),
            params: &PARAMS,
            accent: Color::WHITE,
            threshold_range: (-60.0, 0.0),
        }
    }

    #[test]
    fn dragging_maps_the_plot_width_to_the_transfer_range_and_clamps() {
        let plot = plot();
        let area = TransferPlot::plot(Size::new(324.0, 200.0));
        assert_eq!(plot.threshold_at(area, area.x), -48.0);
        assert_eq!(plot.threshold_at(area, area.x + area.width / 2.0), -24.0);
        assert_eq!(plot.threshold_at(area, area.x + area.width * 2.0), 0.0);
    }

    #[test]
    fn the_threshold_node_sits_on_the_curve() {
        let plot = plot();
        let area = TransferPlot::plot(Size::new(324.0, 200.0));
        let node = plot.node(area);
        assert!((node.x - TransferPlot::x(area, -18.0)).abs() < 1e-3);
        let expected = TransferPlot::y(area, static_transfer_db(-18.0, plot.params));
        assert!((node.y - expected).abs() < 1e-3);
    }
}
