use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use vibez_core::id::TrackId;

/// Horizontal fader widget for track gain control (arrangement track headers).
pub struct HorizontalFaderWidget {
    pub track_id: TrackId,
    /// Current gain value (0.0..2.0).
    pub value: f32,
}

impl HorizontalFaderWidget {
    pub fn new(track_id: TrackId, value: f32) -> Self {
        Self { track_id, value }
    }
}

/// State for horizontal fader mouse dragging.
#[derive(Debug, Default)]
pub struct HorizontalFaderState {
    dragging: bool,
    last_x: f32,
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
            theme::FADER_TRACK,
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
                .with_color(theme::TEXT_DIM)
                .with_width(1.0),
        );

        // Handle position: gain 0.0 = left, gain 2.0 = right
        let normalized = (self.value / 2.0).clamp(0.0, 1.0);
        let handle_x = track_left + track_w * normalized;
        let handle_w = 8.0;
        let handle_h = h - 4.0;
        let handle_y = 2.0;

        // Filled portion (from left to handle)
        let fill_w = handle_x - track_left;
        if fill_w > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(track_left, track_y),
                iced::Size::new(fill_w, track_h),
                theme::ACCENT,
            );
        }

        // Handle
        frame.fill_rectangle(
            iced::Point::new(handle_x - handle_w / 2.0, handle_y),
            iced::Size::new(handle_w, handle_h),
            theme::FADER_HANDLE,
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging || cursor.is_over(bounds) {
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
                if let Some(pos) = cursor.position_in(bounds) {
                    state.dragging = true;
                    state.last_x = pos.x;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let delta = pos.x - state.last_x;
                        state.last_x = pos.x;

                        let track_w = bounds.width - 8.0;
                        let gain_delta = delta / track_w * 2.0;
                        let new_gain = (self.value + gain_delta).clamp(0.0, 2.0);

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SetTrackGain(self.track_id, new_gain)),
                        );
                    }
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

/// Vertical fader widget for track gain control.
pub struct FaderWidget {
    pub track_id: TrackId,
    /// Current gain value (0.0..2.0).
    pub value: f32,
}

impl FaderWidget {
    pub fn new(track_id: TrackId, value: f32) -> Self {
        Self { track_id, value }
    }
}

/// State for mouse dragging.
#[derive(Debug, Default)]
pub struct FaderState {
    dragging: bool,
    last_y: f32,
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
            theme::FADER_TRACK,
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
                .with_color(theme::TEXT_DIM)
                .with_width(1.0),
        );

        // Handle position: gain 0.0 = bottom, gain 2.0 = top
        let normalized = (self.value / 2.0).clamp(0.0, 1.0);
        let handle_y = track_top + track_h * (1.0 - normalized);
        let handle_h = 10.0;
        let handle_w = w - 4.0;
        let handle_x = 2.0;

        // Filled portion (from bottom to handle)
        let fill_h = track_top + track_h - handle_y;
        if fill_h > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(track_x, handle_y),
                iced::Size::new(track_w, fill_h),
                theme::ACCENT,
            );
        }

        // Handle
        frame.fill_rectangle(
            iced::Point::new(handle_x, handle_y - handle_h / 2.0),
            iced::Size::new(handle_w, handle_h),
            theme::FADER_HANDLE,
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging || cursor.is_over(bounds) {
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
                if let Some(pos) = cursor.position_in(bounds) {
                    state.dragging = true;
                    state.last_y = pos.y;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let delta = state.last_y - pos.y;
                        state.last_y = pos.y;

                        let track_h = bounds.height - 8.0;
                        let gain_delta = delta / track_h * 2.0;
                        let new_gain = (self.value + gain_delta).clamp(0.0, 2.0);

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SetTrackGain(self.track_id, new_gain)),
                        );
                    }
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}
