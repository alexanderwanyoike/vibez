use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use vibez_core::id::TrackId;

/// Rotary knob widget for track pan control.
pub struct KnobWidget {
    pub track_id: TrackId,
    /// Current pan value (0.0 = left, 0.5 = center, 1.0 = right).
    pub value: f32,
}

impl KnobWidget {
    pub fn new(track_id: TrackId, value: f32) -> Self {
        Self { track_id, value }
    }
}

/// State for mouse dragging.
#[derive(Debug, Default)]
pub struct KnobState {
    dragging: bool,
    last_y: f32,
}

impl canvas::Program<Message> for KnobWidget {
    type State = KnobState;

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
        let center = iced::Point::new(w / 2.0, h / 2.0);
        let radius = (w.min(h) / 2.0 - 3.0).max(4.0);

        // Background circle
        let bg_circle = canvas::Path::circle(center, radius);
        frame.fill(&bg_circle, theme::KNOB_BG);

        // Arc showing the value
        // Range: from ~225° (left) through 270° (bottom/center) to ~315° (right)
        // Using standard math angles: 225° = 5π/4 (start), 315° = 7π/4 (end)
        let start_angle = std::f32::consts::FRAC_PI_4 * 5.0; // 225°
        let end_angle = std::f32::consts::FRAC_PI_4 * 7.0; // 315°
        let value_angle = start_angle + self.value * (end_angle - start_angle);

        // Draw the arc as line segments
        let arc_radius = radius - 2.0;
        let segments = 32;

        // Background arc (full range)
        let bg_arc = build_arc(center, arc_radius, start_angle, end_angle, segments);
        frame.stroke(
            &bg_arc,
            canvas::Stroke::default()
                .with_color(theme::FADER_TRACK)
                .with_width(2.5),
        );

        // Value arc
        let value_arc = build_arc(center, arc_radius, start_angle, value_angle, segments);
        frame.stroke(
            &value_arc,
            canvas::Stroke::default()
                .with_color(theme::KNOB_ARC)
                .with_width(2.5),
        );

        // Indicator dot at the value position
        let dot_x = center.x + arc_radius * value_angle.cos();
        let dot_y = center.y + arc_radius * value_angle.sin();
        let dot = canvas::Path::circle(iced::Point::new(dot_x, dot_y), 2.5);
        frame.fill(&dot, theme::TEXT);

        // Center marker line
        let center_angle = (start_angle + end_angle) / 2.0;
        let mark_inner = radius - 6.0;
        let mark_outer = radius - 1.0;
        let mark = canvas::Path::line(
            iced::Point::new(
                center.x + mark_inner * center_angle.cos(),
                center.y + mark_inner * center_angle.sin(),
            ),
            iced::Point::new(
                center.x + mark_outer * center_angle.cos(),
                center.y + mark_outer * center_angle.sin(),
            ),
        );
        frame.stroke(
            &mark,
            canvas::Stroke::default()
                .with_color(theme::TEXT_DIM)
                .with_width(1.0),
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
                if cursor.is_over(bounds) {
                    state.dragging = true;
                    if let Some(pos) = cursor.position_in(bounds) {
                        state.last_y = pos.y;
                    }
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

                        let sensitivity = 0.005;
                        let new_pan = (self.value + delta * sensitivity).clamp(0.0, 1.0);

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SetTrackPan(self.track_id, new_pan)),
                        );
                    }
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

fn build_arc(
    center: iced::Point,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
) -> canvas::Path {
    canvas::Path::new(|builder| {
        let step = (end - start) / segments as f32;
        for i in 0..=segments {
            let angle = start + step * i as f32;
            let point = iced::Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            );
            if i == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
    })
}
