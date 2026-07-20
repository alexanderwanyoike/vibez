//! Compact, undo-grouped Swing faders for transport and Instrument controls.

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::domains::perform::PerformMsg;
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme;
use crate::widgets::drag::ValueDrag;
use vibez_core::perform::{SwingAmount, SwingOffset};

#[derive(Debug, Clone, Copy)]
enum SwingTarget {
    Project,
    Track,
}

pub struct SwingFaderWidget {
    target: SwingTarget,
    value: f32,
}

impl SwingFaderWidget {
    pub fn project(value: f32) -> Self {
        Self {
            target: SwingTarget::Project,
            value: value.clamp(SwingAmount::MIN, SwingAmount::MAX),
        }
    }

    pub fn track(value: f32) -> Self {
        Self {
            target: SwingTarget::Track,
            value: value.clamp(SwingOffset::MIN, SwingOffset::MAX),
        }
    }

    fn range(&self) -> (f32, f32) {
        match self.target {
            SwingTarget::Project => (SwingAmount::MIN, SwingAmount::MAX),
            SwingTarget::Track => (SwingOffset::MIN, SwingOffset::MAX),
        }
    }

    fn edit_message(&self, value: f32, gesture: UndoGestureId) -> Message {
        let value = (value * 100.0).round() / 100.0;
        let edit = match self.target {
            SwingTarget::Project => PerformMsg::SetProjectSwing(value),
            SwingTarget::Track => PerformMsg::SetTrackSwingOffset(Some(value)),
        };
        Message::Perform(edit).in_undo_gesture(gesture)
    }
}

#[derive(Debug, Default)]
pub struct SwingFaderState {
    drag: ValueDrag,
    undo_gesture: Option<UndoGestureId>,
}

impl canvas::Program<Message> for SwingFaderWidget {
    type State = SwingFaderState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let track_left = 4.0;
        let track_width = (bounds.width - track_left * 2.0).max(1.0);
        let track_height = 3.0;
        let track_y = (bounds.height - track_height) / 2.0;
        let (minimum, maximum) = self.range();
        let normalized = ((self.value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
        let handle_x = track_left + track_width * normalized;

        frame.fill_rectangle(
            iced::Point::new(track_left, track_y),
            iced::Size::new(track_width, track_height),
            theme::fader_track(),
        );

        if matches!(self.target, SwingTarget::Track) {
            let center_x = track_left + track_width / 2.0;
            frame.fill_rectangle(
                iced::Point::new(center_x, track_y - 3.0),
                iced::Size::new(1.0, track_height + 6.0),
                theme::text_muted(),
            );
        }

        if normalized > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(track_left, track_y),
                iced::Size::new(handle_x - track_left, track_height),
                theme::accent(),
            );
        }

        frame.fill_rectangle(
            iced::Point::new(handle_x - 2.0, 3.0),
            iced::Size::new(4.0, (bounds.height - 6.0).max(4.0)),
            theme::text(),
        );

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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let (minimum, maximum) = self.range();
                let track_width = (bounds.width - 8.0).max(1.0);
                let normalized = ((position.x - 4.0) / track_width).clamp(0.0, 1.0);
                let value = minimum + normalized * (maximum - minimum);
                if state.drag.grab(cursor, bounds, value) {
                    let gesture = UndoGestureId::new();
                    state.undo_gesture = Some(gesture);
                    return (
                        canvas::event::Status::Captured,
                        Some(self.edit_message(value, gesture)),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.release() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (minimum, maximum) = self.range();
                // Dragging changes one native percentage point per pixel. A
                // click still maps the complete profile range onto the rail.
                if let Some(value) = state.drag.drag_to(cursor, 0.01, 0.0, minimum..=maximum) {
                    let gesture = *state.undo_gesture.get_or_insert_with(UndoGestureId::new);
                    return (
                        canvas::event::Status::Captured,
                        Some(self.edit_message(value, gesture)),
                    );
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;
    use iced::{Point, Size};

    fn click(widget: &SwingFaderWidget, x: f32) -> Option<Message> {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(108.0, 18.0));
        let cursor = mouse::Cursor::Available(Point::new(x, 9.0));
        let mut state = SwingFaderState::default();
        widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                cursor,
            )
            .1
    }

    fn perform_edit(message: Option<Message>) -> Option<PerformMsg> {
        match message {
            Some(Message::UndoGesture { edit, .. }) => perform_edit(Some(*edit)),
            Some(Message::Perform(edit)) => Some(edit),
            _ => None,
        }
    }

    fn gesture_id(message: Option<Message>) -> Option<UndoGestureId> {
        match message {
            Some(Message::UndoGesture { id, .. }) => Some(id),
            _ => None,
        }
    }

    #[test]
    fn project_click_maps_the_hundred_pixel_track_to_exact_percentages() {
        for (x, expected) in [(40.0, 0.59), (104.0, 0.75)] {
            let message = click(&SwingFaderWidget::project(0.0), x);
            assert!(matches!(
                perform_edit(message),
                Some(PerformMsg::SetProjectSwing(value))
                    if (value - expected).abs() < f32::EPSILON
            ));
        }
    }

    #[test]
    fn track_click_maps_center_to_zero_offset() {
        let message = click(&SwingFaderWidget::track(0.5), 54.0);
        assert!(matches!(
            perform_edit(message),
            Some(PerformMsg::SetTrackSwingOffset(Some(value))) if value.abs() < f32::EPSILON
        ));
    }

    #[test]
    fn one_drag_keeps_one_undo_gesture_and_the_next_drag_gets_another() {
        let widget = SwingFaderWidget::project(0.0);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(108.0, 18.0));
        let mut state = SwingFaderState::default();
        let at = |x| mouse::Cursor::Available(Point::new(x, 9.0));

        let (_, pressed) = widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            at(20.0),
        );
        let (_, dragged) = widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(40.0, 9.0),
            }),
            bounds,
            at(40.0),
        );
        let first = gesture_id(pressed).expect("press edit is undo grouped");
        assert_eq!(gesture_id(dragged), Some(first));

        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds,
            at(40.0),
        );
        let (_, next_press) = widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            at(70.0),
        );
        assert_ne!(gesture_id(next_press), Some(first));
    }
}
