use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme;
use crate::widgets::drag::ValueDrag;
use vibez_core::id::TrackId;

/// Horizontal fader widget for track gain control (arrangement track headers).
pub struct HorizontalFaderWidget {
    pub track_id: TrackId,
    /// Current gain value (0.0..2.0).
    pub value: f32,
    /// Fill color (track color).
    pub fill_color: Color,
}

impl HorizontalFaderWidget {
    pub fn new(track_id: TrackId, value: f32, fill_color: Color) -> Self {
        Self {
            track_id,
            value,
            fill_color,
        }
    }
}

/// State for horizontal fader mouse dragging.
#[derive(Debug, Default)]
pub struct HorizontalFaderState {
    drag: ValueDrag,
    undo_gesture: Option<UndoGestureId>,
}

impl canvas::Program<Message> for HorizontalFaderWidget {
    type State = HorizontalFaderState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Fader track (horizontal)
        let track_h = 4.0;
        let track_y = (h - track_h) / 2.0;
        let track_left = 4.0;
        let track_w = w - 8.0;

        frame.fill_rectangle(
            iced::Point::new(track_left, track_y),
            iced::Size::new(track_w, track_h),
            theme::fader_track(),
        );

        // Unity gain mark (at 0.5 of the track = gain 1.0)
        let unity_x = track_left + track_w * 0.5;
        let unity_line = canvas::Path::line(
            iced::Point::new(unity_x, track_y - 2.0),
            iced::Point::new(unity_x, track_y + track_h + 2.0),
        );
        frame.stroke(
            &unity_line,
            canvas::Stroke::default()
                .with_color(theme::text_dim())
                .with_width(1.0),
        );

        // Handle position: gain 0.0 = left, gain 2.0 = right
        let normalized = (self.value / 2.0).clamp(0.0, 1.0);
        let handle_x = track_left + track_w * normalized;
        let handle_w = 8.0;
        let handle_h = h - 4.0;
        let handle_y = 2.0;

        // Filled portion (from left to handle) using track color
        let fill_w = handle_x - track_left;
        if fill_w > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(track_left, track_y),
                iced::Size::new(fill_w, track_h),
                self.fill_color,
            );
        }

        // Handle
        frame.fill_rectangle(
            iced::Point::new(handle_x - handle_w / 2.0, handle_y),
            iced::Size::new(handle_w, handle_h),
            theme::fader_handle(),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_active() || cursor.is_over(bounds) {
            mouse::Interaction::ResizingHorizontally
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
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if state.drag.grab(cursor, bounds, self.value) {
                    state.undo_gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if state.drag.release() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let track_w = (bounds.width - 8.0).max(1.0);
                if let Some(gain) = state.drag.drag_to(cursor, 2.0 / track_w, 0.0, 0.0..=2.0) {
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_track_gain(self.track_id, gain).in_undo_gesture(
                                *state.undo_gesture.get_or_insert_with(UndoGestureId::new),
                            ),
                        ),
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
    use crate::domains::arrangement::ArrangementMsg;
    use iced::widget::canvas::Program;
    use iced::{Point, Size};

    fn press(cursor_at: Point) -> (canvas::Event, mouse::Cursor) {
        (
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(cursor_at),
        )
    }

    fn drag(cursor_at: Point) -> (canvas::Event, mouse::Cursor) {
        (
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: cursor_at,
            }),
            mouse::Cursor::Available(cursor_at),
        )
    }

    fn release(cursor_at: Point) -> (canvas::Event, mouse::Cursor) {
        (
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(cursor_at),
        )
    }

    fn gesture_of(message: Option<Message>) -> Option<UndoGestureId> {
        match message {
            Some(Message::UndoGesture { id, .. }) => Some(id),
            _ => None,
        }
    }

    fn gain_of(message: Option<Message>) -> Option<f32> {
        match message {
            Some(Message::UndoGesture { edit, .. }) => gain_of(Some(*edit)),
            Some(Message::Arrangement(ArrangementMsg::SetTrackGain(_, gain))) => Some(gain),
            _ => None,
        }
    }

    #[test]
    fn vertical_fader_keeps_tracking_when_cursor_leaves_bounds() {
        let widget = FaderWidget::new(TrackId::MASTER, 1.0, Color::WHITE);
        let bounds = Rectangle::new(Point::new(100.0, 100.0), Size::new(24.0, 108.0));
        let mut state = FaderState::default();

        let (event, cursor) = press(Point::new(112.0, 150.0));
        widget.update(&mut state, event, bounds, cursor);

        // Drag upward while the cursor has drifted left of the strip.
        let (event, cursor) = drag(Point::new(60.0, 130.0));
        let (_, message) = widget.update(&mut state, event, bounds, cursor);
        let gain = gain_of(message).expect("drag outside bounds must keep tracking");
        assert!(gain > 1.0, "upward drag should raise gain, got {gain}");
    }

    #[test]
    fn vertical_fader_accumulates_moves_between_view_rebuilds() {
        // Several CursorMoved events can share one view rebuild, so
        // the widget's `value` field stays stale across them; the
        // drag must still apply every delta, not just the last one.
        let widget = FaderWidget::new(TrackId::MASTER, 1.0, Color::WHITE);
        let bounds = Rectangle::new(Point::new(100.0, 100.0), Size::new(24.0, 108.0));
        let mut state = FaderState::default();

        let (event, cursor) = press(Point::new(112.0, 150.0));
        widget.update(&mut state, event, bounds, cursor);

        let (event, cursor) = drag(Point::new(112.0, 140.0));
        let (_, first) = widget.update(&mut state, event, bounds, cursor);
        let (event, cursor) = drag(Point::new(112.0, 130.0));
        let (_, second) = widget.update(&mut state, event, bounds, cursor);

        let first = gain_of(first).expect("first move emits");
        let second = gain_of(second).expect("second move emits");
        let step = first - 1.0;
        assert!(step > 0.0);
        let expected = 1.0 + 2.0 * step;
        assert!(
            (second - expected).abs() < 1e-4,
            "second move must include the first delta: got {second}, expected {expected}"
        );
    }

    #[test]
    fn horizontal_fader_keeps_tracking_when_cursor_leaves_bounds() {
        let widget = HorizontalFaderWidget::new(TrackId::MASTER, 1.0, Color::WHITE);
        let bounds = Rectangle::new(Point::new(100.0, 100.0), Size::new(108.0, 16.0));
        let mut state = HorizontalFaderState::default();

        let (event, cursor) = press(Point::new(150.0, 108.0));
        widget.update(&mut state, event, bounds, cursor);

        // Drag right while the cursor has drifted below the strip.
        let (event, cursor) = drag(Point::new(170.0, 140.0));
        let (_, message) = widget.update(&mut state, event, bounds, cursor);
        let gain = gain_of(message).expect("drag outside bounds must keep tracking");
        assert!(gain > 1.0, "rightward drag should raise gain, got {gain}");
    }

    #[test]
    fn separate_fader_drags_get_distinct_undo_gestures() {
        let widget = FaderWidget::new(TrackId::MASTER, 1.0, Color::WHITE);
        let bounds = Rectangle::new(Point::new(100.0, 100.0), Size::new(24.0, 108.0));
        let mut state = FaderState::default();

        let (event, cursor) = press(Point::new(112.0, 150.0));
        widget.update(&mut state, event, bounds, cursor);
        let (event, cursor) = drag(Point::new(112.0, 140.0));
        let (_, first) = widget.update(&mut state, event, bounds, cursor);
        let first = gesture_of(first).expect("first drag emits a grouped edit");

        let (event, cursor) = release(Point::new(112.0, 140.0));
        widget.update(&mut state, event, bounds, cursor);
        let (event, cursor) = press(Point::new(112.0, 150.0));
        widget.update(&mut state, event, bounds, cursor);
        let (event, cursor) = drag(Point::new(112.0, 130.0));
        let (_, second) = widget.update(&mut state, event, bounds, cursor);
        let second = gesture_of(second).expect("second drag emits a grouped edit");

        assert_ne!(first, second);
    }
}

/// Vertical fader widget for track gain control.
pub struct FaderWidget {
    pub track_id: TrackId,
    /// Current gain value (0.0..2.0).
    pub value: f32,
    /// Fill color (track color).
    pub fill_color: Color,
}

impl FaderWidget {
    pub fn new(track_id: TrackId, value: f32, fill_color: Color) -> Self {
        Self {
            track_id,
            value,
            fill_color,
        }
    }
}

/// State for mouse dragging.
#[derive(Debug, Default)]
pub struct FaderState {
    drag: ValueDrag,
    undo_gesture: Option<UndoGestureId>,
}

impl canvas::Program<Message> for FaderWidget {
    type State = FaderState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Fader track
        let track_w = 6.0;
        let track_x = (w - track_w) / 2.0;
        let track_top = 4.0;
        let track_h = h - 8.0;

        frame.fill_rectangle(
            iced::Point::new(track_x, track_top),
            iced::Size::new(track_w, track_h),
            theme::fader_track(),
        );

        // Unity gain mark (at 0.5 of the track = gain 1.0)
        let unity_y = track_top + track_h * 0.5;
        let unity_line = canvas::Path::line(
            iced::Point::new(track_x - 2.0, unity_y),
            iced::Point::new(track_x + track_w + 2.0, unity_y),
        );
        frame.stroke(
            &unity_line,
            canvas::Stroke::default()
                .with_color(theme::text_dim())
                .with_width(1.0),
        );

        // Handle position: gain 0.0 = bottom, gain 2.0 = top
        let normalized = (self.value / 2.0).clamp(0.0, 1.0);
        let handle_y = track_top + track_h * (1.0 - normalized);
        let handle_h = 10.0;
        let handle_w = w - 4.0;
        let handle_x = 2.0;

        // Filled portion (from bottom to handle) using track color
        let fill_h = track_top + track_h - handle_y;
        if fill_h > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(track_x, handle_y),
                iced::Size::new(track_w, fill_h),
                self.fill_color,
            );
        }

        // Handle
        frame.fill_rectangle(
            iced::Point::new(handle_x, handle_y - handle_h / 2.0),
            iced::Size::new(handle_w, handle_h),
            theme::fader_handle(),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_active() || cursor.is_over(bounds) {
            mouse::Interaction::ResizingVertically
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
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if state.drag.grab(cursor, bounds, self.value) {
                    state.undo_gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if state.drag.release() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let track_h = (bounds.height - 8.0).max(1.0);
                if let Some(gain) = state.drag.drag_to(cursor, 0.0, -2.0 / track_h, 0.0..=2.0) {
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_track_gain(self.track_id, gain).in_undo_gesture(
                                *state.undo_gesture.get_or_insert_with(UndoGestureId::new),
                            ),
                        ),
                    );
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}
