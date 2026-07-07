use std::time::Instant;

use iced::keyboard;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use vibez_core::id::TrackId;

/// 270° arc sweep matching DAW standards.
/// Start at 135° (bottom-left), end at 405° (bottom-right).
const ARC_START: f32 = std::f32::consts::FRAC_PI_4 * 3.0; // 135° = 3π/4
const ARC_END: f32 = ARC_START + std::f32::consts::FRAC_PI_2 * 3.0; // 135° + 270° = 405° = 9π/4
const ARC_CENTER: f32 = (ARC_START + ARC_END) / 2.0; // 270° = 3π/2

/// Sensitivity: full 0→1 range over ~150px of vertical drag.
const BASE_SENSITIVITY: f32 = 1.0 / 150.0;
/// Shift modifier makes it 5x finer.
const FINE_DIVISOR: f32 = 5.0;
/// Scroll step per line tick.
const SCROLL_STEP: f32 = 0.02;
/// Double-click window in milliseconds.
const DOUBLE_CLICK_MS: u64 = 300;

/// Rotary knob widget for track pan control.
pub struct KnobWidget {
    pub track_id: TrackId,
    /// Current pan value (0.0 = left, 0.5 = center, 1.0 = right).
    pub value: f32,
    /// Arc color (track color).
    pub arc_color: Color,
}

impl KnobWidget {
    pub fn new(track_id: TrackId, value: f32, arc_color: Color) -> Self {
        Self {
            track_id,
            value,
            arc_color,
        }
    }
}

/// State for mouse interaction.
#[derive(Debug)]
pub struct KnobState {
    dragging: bool,
    last_y: f32,
    shift_held: bool,
    last_click: Option<Instant>,
}

impl Default for KnobState {
    fn default() -> Self {
        Self {
            dragging: false,
            last_y: 0.0,
            shift_held: false,
            last_click: None,
        }
    }
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

        let arc_radius = radius - 2.0;

        // Background arc (full 270° range)
        let bg_arc = build_arc(center, arc_radius, ARC_START, ARC_END);
        frame.stroke(
            &bg_arc,
            canvas::Stroke::default()
                .with_color(theme::FADER_TRACK)
                .with_width(3.0),
        );

        // Value arc (filled portion) using track color
        let value_angle = ARC_START + self.value * (ARC_END - ARC_START);
        if self.value > 0.005 {
            let value_arc = build_arc(center, arc_radius, ARC_START, value_angle);
            frame.stroke(
                &value_arc,
                canvas::Stroke::default()
                    .with_color(self.arc_color)
                    .with_width(3.0),
            );
        }

        // Pointer line from center toward current value angle
        let pointer_inner = radius * 0.25;
        let pointer_outer = radius - 3.0;
        let pointer = canvas::Path::line(
            iced::Point::new(
                center.x + pointer_inner * value_angle.cos(),
                center.y + pointer_inner * value_angle.sin(),
            ),
            iced::Point::new(
                center.x + pointer_outer * value_angle.cos(),
                center.y + pointer_outer * value_angle.sin(),
            ),
        );
        frame.stroke(
            &pointer,
            canvas::Stroke::default()
                .with_color(theme::TEXT)
                .with_width(2.0),
        );

        // Center tick mark at bottom (270° / 3π/2)
        let tick_inner = radius - 1.0;
        let tick_outer = radius + 2.0;
        let tick = canvas::Path::line(
            iced::Point::new(
                center.x + tick_inner * ARC_CENTER.cos(),
                center.y + tick_inner * ARC_CENTER.sin(),
            ),
            iced::Point::new(
                center.x + tick_outer * ARC_CENTER.cos(),
                center.y + tick_outer * ARC_CENTER.sin(),
            ),
        );
        frame.stroke(
            &tick,
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
        if state.dragging {
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
            // Track modifier keys
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                return (canvas::event::Status::Ignored, None);
            }

            // Click: start drag or double-click to reset
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    let now = Instant::now();

                    // Double-click detection
                    if let Some(last) = state.last_click {
                        if now.duration_since(last).as_millis() < DOUBLE_CLICK_MS as u128 {
                            state.last_click = None;
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::set_track_pan(self.track_id, 0.5)),
                            );
                        }
                    }
                    state.last_click = Some(now);

                    state.dragging = true;
                    if let Some(pos) = cursor.position() {
                        state.last_y = pos.y;
                    }
                    return (canvas::event::Status::Captured, None);
                }
            }

            // Release
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    return (canvas::event::Status::Captured, None);
                }
            }

            // Drag: vertical movement adjusts value (use absolute position
            // so dragging outside the small knob bounds still tracks)
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(pos) = cursor.position() {
                        let delta = state.last_y - pos.y; // up = positive
                        state.last_y = pos.y;

                        let sensitivity = if state.shift_held {
                            BASE_SENSITIVITY / FINE_DIVISOR
                        } else {
                            BASE_SENSITIVITY
                        };

                        let new_pan = (self.value + delta * sensitivity).clamp(0.0, 1.0);

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::set_track_pan(self.track_id, new_pan)),
                        );
                    }
                }
            }

            // Scroll wheel
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) =>
            {
                let scroll_y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
                };

                let step = if state.shift_held {
                    SCROLL_STEP / FINE_DIVISOR
                } else {
                    SCROLL_STEP
                };

                let new_pan = (self.value + scroll_y * step).clamp(0.0, 1.0);

                return (
                    canvas::event::Status::Captured,
                    Some(Message::set_track_pan(self.track_id, new_pan)),
                );
            }

            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

fn build_arc(center: iced::Point, radius: f32, start: f32, end: f32) -> canvas::Path {
    // Native arc geometry: true curves stay antialiased at any size,
    // unlike the segment polylines that caused visible stair-step
    // artifacting on small knobs.
    canvas::Path::new(|builder| {
        builder.arc(canvas::path::Arc {
            center,
            radius,
            start_angle: iced::Radians(start),
            end_angle: iced::Radians(end),
        });
    })
}
