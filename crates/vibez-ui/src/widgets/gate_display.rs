//! The Gate's history: what its detector heard over the last few seconds,
//! when it was open and how far it attenuated, under a threshold to drag.

use std::time::Instant;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::{EffectId, TrackId};
use vibez_dsp::gate::{param, GATE_PARAMS};

use crate::domains::device_levels::ReadingHistory;
use crate::domains::gate::{GateReading, HISTORY};
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme as th;

const FLOOR_DB: f32 = -60.0;
const LABEL_W: f32 = 24.0;
const LABEL_H: f32 = 14.0;
/// How close to the threshold line a press picks it up.
const GRAB: f32 = 6.0;

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

pub struct GateHistory<'a> {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub params: &'a [f32],
    pub readings: &'a ReadingHistory<GateReading>,
    pub now: Instant,
    pub accent: Color,
}

#[derive(Debug, Default)]
pub struct HistoryState {
    gesture: Option<UndoGestureId>,
}

impl GateHistory<'_> {
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

    fn y(plot: Rectangle, db: f32) -> f32 {
        plot.y + db.clamp(FLOOR_DB, 0.0) / FLOOR_DB * plot.height
    }

    fn db_at(plot: Rectangle, y: f32) -> f32 {
        let range = &GATE_PARAMS[param::THRESHOLD];
        (((y - plot.y) / plot.height).clamp(0.0, 1.0) * FLOOR_DB).clamp(range.min, range.max)
    }

    fn threshold(&self) -> f32 {
        self.params
            .get(param::THRESHOLD)
            .copied()
            .unwrap_or(GATE_PARAMS[param::THRESHOLD].default)
    }

    fn near_threshold(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        let plot = Self::plot(bounds.size());
        cursor.position_in(bounds).is_some_and(|at| {
            plot.contains(at) && (at.y - Self::y(plot, self.threshold())).abs() <= GRAB
        })
    }
}

impl canvas::Program<Message> for GateHistory<'_> {
    type State = HistoryState;

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
        for db in [0.0f32, -12.0, -24.0, -36.0, -48.0, -60.0] {
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
        let span = HISTORY.as_secs_f32();
        let seconds = span as usize;
        for second in 0..=seconds {
            let x = plot.x + plot.width * (1.0 - second as f32 / span);
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels {
                let (text, horizontal) = match second {
                    0 => ("now".to_string(), iced::alignment::Horizontal::Right),
                    s if s == seconds => (format!("-{s}s"), iced::alignment::Horizontal::Left),
                    s => (format!("-{s}s"), iced::alignment::Horizontal::Center),
                };
                label(
                    &mut frame,
                    text,
                    Point::new(x, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    horizontal,
                );
            }
        }
        let x = |at: Instant| {
            let age = self.now.saturating_duration_since(at).as_secs_f32();
            plot.x + plot.width * (1.0 - age / span).clamp(0.0, 1.0)
        };
        let threshold_y = Self::y(plot, self.threshold());
        let open_top = plot.y + 4.0;
        let mut previous: Option<f32> = None;
        let mut runs: Vec<(f32, f32)> = Vec::new();
        for (at, reading) in self.readings.iter() {
            let right = x(*at);
            let left = previous.unwrap_or(right);
            previous = Some(right);
            if right <= left {
                continue;
            }
            if reading.open {
                match runs.last_mut() {
                    Some((_, end)) if *end >= left => *end = right,
                    _ => runs.push((left, right)),
                }
            } else if reading.reduction_db > 0.5 {
                let depth = (reading.reduction_db / -FLOOR_DB).min(1.0);
                frame.fill_rectangle(
                    Point::new(left, threshold_y),
                    Size::new(right - left, plot.y + plot.height - threshold_y),
                    Color {
                        a: 0.04 + 0.10 * depth,
                        ..self.accent
                    },
                );
            }
        }
        // Each opening reads as one box rising from the threshold.
        for (left, right) in runs {
            let size = Size::new(right - left, (threshold_y - open_top).max(0.0));
            frame.fill_rectangle(
                Point::new(left, open_top),
                size,
                Color {
                    a: 0.12,
                    ..self.accent
                },
            );
            frame.stroke(
                &canvas::Path::new(|builder| {
                    builder.move_to(Point::new(left, threshold_y));
                    builder.line_to(Point::new(left, open_top));
                    builder.line_to(Point::new(right, open_top));
                    builder.line_to(Point::new(right, threshold_y));
                }),
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.5),
            );
        }
        if self.readings.len() > 1 {
            let trace = canvas::Path::new(|builder| {
                for (index, (at, reading)) in self.readings.iter().enumerate() {
                    let point = Point::new(x(*at), Self::y(plot, reading.level_db));
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });
            frame.stroke(
                &trace,
                canvas::Stroke::default()
                    .with_color(th::text_dim())
                    .with_width(1.0),
            );
        }
        let engaged = state.gesture.is_some() || self.near_threshold(bounds, cursor);
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, threshold_y),
                Point::new(plot.x + plot.width, threshold_y),
            ),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(if engaged { 2.5 } else { 1.5 }),
        );
        frame.fill_text(canvas::Text {
            content: format!("{:.0} dB", self.threshold()),
            position: Point::new(plot.x + 4.0, (threshold_y - 8.0).max(plot.y + 6.0)),
            color: th::text(),
            size: 10.0.into(),
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
                if self.near_threshold(bounds, cursor) =>
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
                    let threshold = Self::db_at(Self::plot(bounds.size()), at.y - bounds.y);
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
        if state.gesture.is_some() {
            mouse::Interaction::Grabbing
        } else if self.near_threshold(bounds, cursor) {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressing_the_threshold_line_and_dragging_sets_the_threshold() {
        use canvas::Program;
        let params: Vec<f32> = GATE_PARAMS.iter().map(|d| d.default).collect();
        let readings = ReadingHistory::default();
        let history = GateHistory {
            track_id: TrackId::new(),
            effect_id: EffectId::new(),
            params: &params,
            readings: &readings,
            now: Instant::now(),
            accent: Color::WHITE,
        };
        let bounds = Rectangle::new(Point::new(300.0, 780.0), Size::new(440.0, 100.0));
        let plot = GateHistory::plot(bounds.size());
        let line = Point::new(
            bounds.x + 200.0,
            bounds.y + GateHistory::y(plot, params[param::THRESHOLD]),
        );
        let mut state = HistoryState::default();
        let (status, _) = history.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(line),
        );
        assert_eq!(status, canvas::event::Status::Captured);
        let (_, message) = history.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(line.x, line.y - 20.0),
            }),
            bounds,
            mouse::Cursor::Available(Point::new(line.x, line.y - 20.0)),
        );
        assert!(message.is_some());
    }

    #[test]
    fn dragging_maps_the_plot_height_to_the_threshold_and_clamps() {
        let plot = GateHistory::plot(Size::new(400.0, 200.0));
        assert_eq!(GateHistory::db_at(plot, plot.y), 0.0);
        assert_eq!(GateHistory::db_at(plot, plot.y + plot.height), FLOOR_DB);
        assert_eq!(
            GateHistory::db_at(plot, plot.y + plot.height * 4.0),
            FLOOR_DB
        );
        let y = GateHistory::y(plot, -32.0);
        assert!((GateHistory::db_at(plot, y) + 32.0).abs() < 1e-3);
    }
}
