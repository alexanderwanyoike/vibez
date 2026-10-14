//! Interactive Filter response using the same coefficients as audio processing.

use crate::domains::device_controls::{log_position, log_value};
use crate::{
    message::Message,
    state::{UiEffect, UndoGestureId},
    theme as th,
};
use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::id::TrackId;
use vibez_dsp::filter::{
    param, response_magnitude, CUTOFF_MAX, CUTOFF_MIN, RESONANCE_MAX, RESONANCE_MIN,
};

pub struct FilterResponse<'a> {
    pub track: TrackId,
    pub effect: &'a UiEffect,
    pub sample_rate: f32,
    pub accent: Color,
}

#[derive(Default)]
pub struct ResponseState {
    drag: Option<(UndoGestureId, f32, f32)>,
}

impl FilterResponse<'_> {
    fn value(&self, index: usize) -> f32 {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(self.effect.descriptors[index].default)
    }
    fn magnitude(&self, frequency: f32) -> f32 {
        if self.effect.bypass {
            return 1.0;
        }
        response_magnitude(
            self.sample_rate,
            self.value(param::CUTOFF),
            self.value(param::RESONANCE),
            self.value(param::MODE),
            self.value(param::SLOPE),
            self.value(param::MIX),
            frequency,
        )
    }
    fn frequency(&self, x: f32, width: f32) -> f32 {
        log_value((x - 26.0) / (width - 34.0).max(1.0), CUTOFF_MIN, CUTOFF_MAX)
    }
    fn db_y(db: f32, height: f32) -> f32 {
        let (top, span) = if height < 48.0 {
            (2.0, (height - 4.0).max(1.0))
        } else {
            (4.0, (height - 18.0).max(1.0))
        };
        top + (12.0 - db.clamp(-48.0, 12.0)) / 60.0 * span
    }
}

impl canvas::Program<Message> for FilterResponse<'_> {
    type State = ResponseState;

    fn draw(
        &self,
        _: &ResponseState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
        let plot_w = (bounds.width - 34.0).max(1.0);
        let x = |frequency| 26.0 + log_position(frequency, CUTOFF_MIN, CUTOFF_MAX) * plot_w;
        let y = |db| Self::db_y(db, bounds.height);
        for frequency in [20.0, 100.0, 1000.0, 10000.0, 20000.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(frequency), 0.0),
                    Point::new(x(frequency), bounds.height - 13.0),
                ),
                canvas::Stroke::default().with_color(th::divider()),
            );
            if bounds.height >= 48.0 {
                frame.fill_text(canvas::Text {
                    content: if frequency >= 1000.0 {
                        format!("{}k", frequency / 1000.0)
                    } else {
                        format!("{frequency:.0}")
                    },
                    position: Point::new(x(frequency), bounds.height),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Center,
                    vertical_alignment: iced::alignment::Vertical::Bottom,
                    ..Default::default()
                });
            }
        }
        for db in [12.0, 0.0, -12.0, -24.0, -48.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(26.0, y(db)),
                    Point::new(bounds.width - 8.0, y(db)),
                ),
                canvas::Stroke::default().with_color(th::divider()),
            );
            if bounds.height >= 90.0 || (bounds.height >= 48.0 && (db == 0.0 || db == -48.0)) {
                frame.fill_text(canvas::Text {
                    content: format!("{db:.0}"),
                    position: Point::new(23.0, y(db)),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
        let path = canvas::Path::new(|builder| {
            for index in 0..180 {
                let frequency = log_value(index as f32 / 179.0, CUTOFF_MIN, CUTOFF_MAX);
                let db = vibez_core::level::amplitude_to_db(self.magnitude(frequency));
                let point = Point::new(x(frequency), y(db));
                if index == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        let accent = if self.effect.bypass {
            th::text_muted()
        } else {
            self.accent
        };
        let fill = canvas::Path::new(|builder| {
            builder.move_to(Point::new(26.0, y(-48.0)));
            for index in 0..180 {
                let frequency = log_value(index as f32 / 179.0, CUTOFF_MIN, CUTOFF_MAX);
                let db = vibez_core::level::amplitude_to_db(self.magnitude(frequency));
                builder.line_to(Point::new(x(frequency), y(db)));
            }
            builder.line_to(Point::new(bounds.width - 8.0, y(-48.0)));
            builder.close();
        });
        frame.fill(&fill, th::with_alpha(accent, 0.08));
        frame.stroke(
            &path,
            canvas::Stroke::default().with_color(accent).with_width(1.8),
        );
        let cutoff = self.value(param::CUTOFF);
        frame.fill(
            &canvas::Path::circle(
                Point::new(
                    x(cutoff),
                    y(vibez_core::level::amplitude_to_db(self.magnitude(cutoff))),
                ),
                4.0,
            ),
            accent,
        );
        if bounds.height >= 54.0 && bounds.width >= 120.0 {
            let shown = if cutoff >= 1000.0 {
                format!("{:.2} kHz", cutoff / 1000.0)
            } else {
                format!("{cutoff:.0} Hz")
            };
            let node_y = y(vibez_core::level::amplitude_to_db(self.magnitude(cutoff)));
            frame.fill_text(canvas::Text {
                content: shown,
                position: Point::new(
                    x(cutoff).clamp(56.0, bounds.width - 38.0),
                    (node_y - 12.0).clamp(8.0, bounds.height - 26.0),
                ),
                color: th::text(),
                size: 9.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center,
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut ResponseState,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let captured = canvas::event::Status::Captured;
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let gesture = UndoGestureId::new();
                    state.drag = Some((gesture, position.y, self.value(param::RESONANCE)));
                    return (
                        captured,
                        Some(
                            Message::set_effect_param(
                                self.track,
                                self.effect.id,
                                param::CUTOFF,
                                self.frequency(position.x, bounds.width),
                            )
                            .in_undo_gesture(gesture),
                        ),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((gesture, start_y, start_q)) = state.drag {
                    if let Some(position) = cursor.position() {
                        let position = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let q = (start_q
                            * ((start_y - position.y) / bounds.height.max(1.0) * 4.0).exp())
                        .clamp(RESONANCE_MIN, RESONANCE_MAX);
                        return (
                            captured,
                            Some(
                                Message::set_effect_params(
                                    self.track,
                                    self.effect.id,
                                    vec![
                                        (param::CUTOFF, self.frequency(position.x, bounds.width)),
                                        (param::RESONANCE, q),
                                    ],
                                )
                                .in_undo_gesture(gesture),
                            ),
                        );
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                return (captured, None)
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }
    fn mouse_interaction(
        &self,
        state: &ResponseState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_response_uses_its_available_vertical_range() {
        for height in [10.0, 14.0, 24.0, 40.0] {
            let top = FilterResponse::db_y(12.0, height);
            let bottom = FilterResponse::db_y(-48.0, height);
            assert!(top >= 0.0 && bottom <= height);
            assert!(
                bottom - top >= height - 4.0,
                "curve is compressed at {height}px"
            );
        }
    }

    #[test]
    fn response_scale_keeps_unity_and_floor_within_the_plot() {
        for height in [24.0, 60.0, 200.0] {
            assert!(FilterResponse::db_y(0.0, height) >= 0.0);
            assert!(FilterResponse::db_y(-48.0, height) < height);
            assert!(FilterResponse::db_y(12.0, height) < FilterResponse::db_y(0.0, height));
        }
    }
}
