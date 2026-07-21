//! Compact, undo-grouped Swing knobs for Project and contextual Track Swing.

use iced::keyboard;
use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::domains::perform::PerformMsg;
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme;
use crate::widgets::drag::ValueDrag;
use vibez_core::perform::{SwingAmount, SwingOffset};

const ARC_START: f32 = std::f32::consts::FRAC_PI_4 * 3.0;
const ARC_END: f32 = ARC_START + std::f32::consts::FRAC_PI_2 * 3.0;
const NORMAL_SENSITIVITY: f32 = 0.005;
const FINE_SENSITIVITY: f32 = 0.001;
const STEP: f32 = 0.01;

pub(crate) fn parse_swing_percent(input: &str) -> Option<SwingAmount> {
    let percent = input
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<f32>()
        .ok()?;
    let value = percent / 100.0;
    (value.is_finite() && (SwingAmount::MIN..=SwingAmount::MAX).contains(&value))
        .then(|| SwingAmount::new(value))
}

pub(crate) fn offset_for_effective_percent(
    input: &str,
    project: SwingAmount,
) -> Option<SwingOffset> {
    parse_swing_percent(input).map(|effective| SwingOffset::new(effective.get() - project.get()))
}

#[derive(Debug, Clone, Copy)]
enum SwingTarget {
    Project,
    Track { project: SwingAmount },
}

pub struct SwingKnobWidget {
    target: SwingTarget,
    effective_value: f32,
    automated: bool,
}

impl SwingKnobWidget {
    pub fn project(value: SwingAmount) -> Self {
        Self {
            target: SwingTarget::Project,
            effective_value: value.get(),
            automated: false,
        }
    }

    pub fn track(project: SwingAmount, effective: SwingAmount, automated: bool) -> Self {
        Self {
            target: SwingTarget::Track { project },
            effective_value: effective.get(),
            automated,
        }
    }

    fn normalized(&self) -> f32 {
        ((self.effective_value - SwingAmount::MIN) / (SwingAmount::MAX - SwingAmount::MIN))
            .clamp(0.0, 1.0)
    }

    fn edit_message(&self, effective_value: f32, gesture: Option<UndoGestureId>) -> Message {
        let effective_value =
            ((effective_value * 100.0).round() / 100.0).clamp(SwingAmount::MIN, SwingAmount::MAX);
        let edit = match self.target {
            SwingTarget::Project => PerformMsg::SetProjectSwing(effective_value),
            SwingTarget::Track { project } => PerformMsg::SetTrackSwingOffset(Some(
                SwingOffset::new(effective_value - project.get()).get(),
            )),
        };
        let message = Message::Perform(edit);
        match gesture {
            Some(gesture) => message.in_undo_gesture(gesture),
            None => message,
        }
    }
}

#[derive(Debug, Default)]
pub struct SwingKnobState {
    drag: ValueDrag,
    undo_gesture: Option<UndoGestureId>,
    shift_held: bool,
}

impl canvas::Program<Message> for SwingKnobWidget {
    type State = SwingKnobState;

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
        let radius = (bounds.width.min(bounds.height) / 2.0 - 2.0).max(6.0);
        let arc_radius = radius - 1.0;
        let body_radius = radius - 4.5;
        let value_angle = ARC_START + self.normalized() * (ARC_END - ARC_START);
        let engaged = state.drag.is_active() || cursor.is_over(bounds);
        let round = canvas::Stroke {
            line_cap: canvas::LineCap::Round,
            ..canvas::Stroke::default()
        };

        frame.stroke(
            &build_arc(center, arc_radius, ARC_START, ARC_END),
            round.with_color(theme::knob_track()).with_width(2.5),
        );
        frame.stroke(
            &build_arc(center, arc_radius, ARC_START, value_angle),
            round
                .with_color(if self.automated {
                    theme::success()
                } else {
                    theme::accent()
                })
                .with_width(if self.automated { 3.0 } else { 2.5 }),
        );

        let body = canvas::Path::circle(center, body_radius);
        frame.fill(
            &body,
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
        if self.automated {
            mouse::Interaction::default()
        } else if state.drag.is_active() {
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
        if self.automated {
            return (canvas::event::Status::Ignored, None);
        }
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                return (canvas::event::Status::Ignored, None);
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if state.drag.grab(cursor, bounds, self.effective_value) {
                    state.undo_gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.release() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let sensitivity = if state.shift_held {
                    FINE_SENSITIVITY
                } else {
                    NORMAL_SENSITIVITY
                };
                if let Some(value) = state.drag.drag_to(
                    cursor,
                    0.0,
                    -sensitivity,
                    SwingAmount::MIN..=SwingAmount::MAX,
                ) {
                    let gesture = *state.undo_gesture.get_or_insert_with(UndoGestureId::new);
                    return (
                        canvas::event::Status::Captured,
                        Some(self.edit_message(value, Some(gesture))),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) =>
            {
                let direction = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y.signum(),
                    mouse::ScrollDelta::Pixels { y, .. } => y.signum(),
                };
                return (
                    canvas::event::Status::Captured,
                    Some(self.edit_message(self.effective_value + direction * STEP, None)),
                );
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
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
    use iced::widget::canvas::Program;
    use iced::{Point, Size};

    fn perform_edit(message: Option<Message>) -> Option<PerformMsg> {
        match message {
            Some(Message::UndoGesture { edit, .. }) => perform_edit(Some(*edit)),
            Some(Message::Perform(edit)) => Some(edit),
            _ => None,
        }
    }

    fn gesture_id(message: &Option<Message>) -> Option<UndoGestureId> {
        match message {
            Some(Message::UndoGesture { id, .. }) => Some(*id),
            _ => None,
        }
    }

    #[test]
    fn project_drag_reaches_exact_native_percentages() {
        let widget = SwingKnobWidget::project(SwingAmount::STRAIGHT);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(40.0, 40.0));
        let mut state = SwingKnobState::default();
        let at = |y| mouse::Cursor::Available(Point::new(20.0, y));
        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            at(20.0),
        );
        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(20.0, 2.0),
                }),
                bounds,
                at(2.0),
            )
            .1;
        assert!(matches!(
            perform_edit(message),
            Some(PerformMsg::SetProjectSwing(value)) if (value - 0.59).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn target_knob_emits_a_project_relative_offset_from_its_effective_value() {
        let widget = SwingKnobWidget::track(SwingAmount::new(0.56), SwingAmount::new(0.63), false);
        let message = widget.edit_message(0.63, None);
        assert!(matches!(
            perform_edit(Some(message)),
            Some(PerformMsg::SetTrackSwingOffset(Some(value)))
                if (value - 0.07).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn one_pointer_drag_reuses_one_undo_gesture() {
        let widget = SwingKnobWidget::project(SwingAmount::STRAIGHT);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(40.0, 40.0));
        let mut state = SwingKnobState::default();
        let at = |y| mouse::Cursor::Available(Point::new(20.0, y));
        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            at(20.0),
        );
        let first = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(20.0, 18.0),
                }),
                bounds,
                at(18.0),
            )
            .1;
        let second = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(20.0, 16.0),
                }),
                bounds,
                at(16.0),
            )
            .1;
        assert!(gesture_id(&first).is_some());
        assert_eq!(gesture_id(&first), gesture_id(&second));
    }

    #[test]
    fn numeric_entry_accepts_exact_native_percentages_and_rejects_out_of_range_values() {
        assert_eq!(parse_swing_percent("59%"), Some(SwingAmount::new(0.59)));
        assert_eq!(parse_swing_percent(" 75 "), Some(SwingAmount::new(0.75)));
        assert_eq!(parse_swing_percent("49"), None);
        assert_eq!(parse_swing_percent("76"), None);
        assert_eq!(parse_swing_percent("nope"), None);
        let offset = offset_for_effective_percent("63", SwingAmount::new(0.56)).unwrap();
        assert!((offset.get() - 0.07).abs() < 1.0e-6);
    }

    #[test]
    fn automated_target_knob_is_read_only() {
        let widget = SwingKnobWidget::track(SwingAmount::new(0.56), SwingAmount::new(0.63), true);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(40.0, 40.0));
        let mut state = SwingKnobState::default();
        let cursor = mouse::Cursor::Available(Point::new(20.0, 20.0));
        let result = widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            cursor,
        );
        assert_eq!(result.0, canvas::event::Status::Ignored);
        assert!(result.1.is_none());
    }
}
