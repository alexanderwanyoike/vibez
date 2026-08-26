//! Continuous transient-analysis sensitivity control.

use iced::keyboard;
use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use crate::widgets::drag::ValueDrag;

const ARC_START: f32 = std::f32::consts::FRAC_PI_4 * 3.0;
const ARC_END: f32 = ARC_START + std::f32::consts::FRAC_PI_2 * 3.0;
const NORMAL_SENSITIVITY: f32 = 1.0;
const FINE_SENSITIVITY: f32 = 0.2;

pub struct TransientSensitivityKnob {
    percent: u8,
}

impl TransientSensitivityKnob {
    pub fn new(percent: u8) -> Self {
        Self {
            percent: percent.min(100),
        }
    }

    fn message(percent: f32) -> Message {
        Message::SetTransientAnalysisSensitivity(percent.round().clamp(0.0, 100.0) as u8)
    }
}

#[derive(Debug, Default)]
pub struct TransientSensitivityKnobState {
    drag: ValueDrag,
    shift_held: bool,
}

impl canvas::Program<Message> for TransientSensitivityKnob {
    type State = TransientSensitivityKnobState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = iced::Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = (bounds.width.min(bounds.height) / 2.0 - 2.0).max(7.0);
        let arc_radius = radius - 1.0;
        let body_radius = radius - 5.5;
        let normalized = self.percent as f32 / 100.0;
        let value_angle = ARC_START + normalized * (ARC_END - ARC_START);
        let engaged = state.drag.is_active() || cursor.is_over(bounds);
        let round = canvas::Stroke {
            line_cap: canvas::LineCap::Round,
            ..canvas::Stroke::default()
        };

        frame.stroke(
            &build_arc(center, arc_radius, ARC_START, ARC_END),
            round.with_color(theme::knob_track()).with_width(3.0),
        );
        frame.stroke(
            &build_arc(center, arc_radius, ARC_START, value_angle),
            round.with_color(theme::accent()).with_width(3.0),
        );

        frame.fill(
            &canvas::Path::circle(center, body_radius),
            if engaged {
                theme::knob_body_engaged()
            } else {
                theme::knob_body()
            },
        );
        let pointer = canvas::Path::line(
            iced::Point::new(
                center.x + body_radius * 0.3 * value_angle.cos(),
                center.y + body_radius * 0.3 * value_angle.sin(),
            ),
            iced::Point::new(
                center.x + (body_radius - 1.0) * value_angle.cos(),
                center.y + (body_radius - 1.0) * value_angle.sin(),
            ),
        );
        frame.stroke(&pointer, round.with_color(theme::text()).with_width(2.0));

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_active() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                (canvas::event::Status::Ignored, None)
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if state.drag.grab(cursor, bounds, self.percent as f32) {
                    (canvas::event::Status::Captured, None)
                } else {
                    (canvas::event::Status::Ignored, None)
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.release() {
                    (canvas::event::Status::Captured, None)
                } else {
                    (canvas::event::Status::Ignored, None)
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let sensitivity = if state.shift_held {
                    FINE_SENSITIVITY
                } else {
                    NORMAL_SENSITIVITY
                };
                let message = state
                    .drag
                    .drag_to(cursor, 0.0, -sensitivity, 0.0..=100.0)
                    .map(Self::message);
                let status = if message.is_some() {
                    canvas::event::Status::Captured
                } else {
                    canvas::event::Status::Ignored
                };
                (status, message)
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) =>
            {
                let direction = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y.signum(),
                    mouse::ScrollDelta::Pixels { y, .. } => y.signum(),
                };
                (
                    canvas::event::Status::Captured,
                    Some(Self::message(self.percent as f32 + direction)),
                )
            }
            _ => (canvas::event::Status::Ignored, None),
        }
    }
}

fn build_arc(center: iced::Point, radius: f32, start: f32, end: f32) -> canvas::Path {
    canvas::Path::new(|builder| {
        builder.arc(canvas::path::Arc {
            center,
            radius,
            start_angle: iced::Radians(start),
            end_angle: iced::Radians(end),
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_clamps_messages_to_an_exact_percentage() {
        assert!(matches!(
            TransientSensitivityKnob::message(71.6),
            Message::SetTransientAnalysisSensitivity(72)
        ));
        assert!(matches!(
            TransientSensitivityKnob::message(200.0),
            Message::SetTransientAnalysisSensitivity(100)
        ));
    }
}
