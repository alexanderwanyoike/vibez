use std::time::Instant;

use iced::keyboard;
use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::theme;
use vibez_core::id::{EffectId, TrackId};

/// 270-degree arc sweep matching DAW standards.
const ARC_START: f32 = std::f32::consts::FRAC_PI_4 * 3.0; // 135 degrees
const ARC_END: f32 = ARC_START + std::f32::consts::FRAC_PI_2 * 3.0; // 405 degrees

/// Sensitivity: full 0-1 range over ~150px of vertical drag.
const BASE_SENSITIVITY: f32 = 1.0 / 150.0;
/// Shift modifier makes it 5x finer.
const FINE_DIVISOR: f32 = 5.0;
/// Scroll step per line tick.
const SCROLL_STEP: f32 = 0.02;
/// Double-click window in milliseconds.
const DOUBLE_CLICK_MS: u64 = 300;

/// Generalized rotary knob widget for effect parameters with arbitrary min/max.
pub struct EffectKnobWidget {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub param_index: usize,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl EffectKnobWidget {
    pub fn new(
        track_id: TrackId,
        effect_id: EffectId,
        param_index: usize,
        value: f32,
        min: f32,
        max: f32,
        default: f32,
    ) -> Self {
        Self {
            track_id,
            effect_id,
            param_index,
            value,
            min,
            max,
            default,
        }
    }

    /// Normalize value to 0.0..1.0 range.
    fn normalized(&self) -> f32 {
        let range = self.max - self.min;
        if range <= 0.0 {
            0.0
        } else {
            ((self.value - self.min) / range).clamp(0.0, 1.0)
        }
    }

    /// Denormalize from 0.0..1.0 to actual value range.
    fn denormalize(&self, n: f32) -> f32 {
        self.min + n.clamp(0.0, 1.0) * (self.max - self.min)
    }
}

/// State for mouse interaction.
#[derive(Debug, Default)]
pub struct EffectKnobState {
    dragging: bool,
    last_y: f32,
    shift_held: bool,
    last_click: Option<Instant>,
}

impl canvas::Program<Message> for EffectKnobWidget {
    type State = EffectKnobState;

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
        let segments = 40;
        let norm = self.normalized();

        // Background arc (full 270-degree range)
        let bg_arc = build_arc(center, arc_radius, ARC_START, ARC_END, segments);
        frame.stroke(
            &bg_arc,
            canvas::Stroke::default()
                .with_color(theme::FADER_TRACK)
                .with_width(3.0),
        );

        // Value arc (filled portion)
        let value_angle = ARC_START + norm * (ARC_END - ARC_START);
        if norm > 0.005 {
            let value_arc = build_arc(center, arc_radius, ARC_START, value_angle, segments);
            frame.stroke(
                &value_arc,
                canvas::Stroke::default()
                    .with_color(theme::KNOB_ARC)
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

                    // Double-click detection: reset to default
                    if let Some(last) = state.last_click {
                        if now.duration_since(last).as_millis() < DOUBLE_CLICK_MS as u128 {
                            state.last_click = None;
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::SetEffectParam(
                                    self.track_id,
                                    self.effect_id,
                                    self.param_index,
                                    self.default,
                                )),
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

            // Drag: vertical movement adjusts value
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

                        let norm = self.normalized();
                        let new_norm = (norm + delta * sensitivity).clamp(0.0, 1.0);
                        let new_value = self.denormalize(new_norm);

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SetEffectParam(
                                self.track_id,
                                self.effect_id,
                                self.param_index,
                                new_value,
                            )),
                        );
                    }
                }
            }

            // Scroll wheel
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let scroll_y = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y,
                        mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
                    };

                    let step = if state.shift_held {
                        SCROLL_STEP / FINE_DIVISOR
                    } else {
                        SCROLL_STEP
                    };

                    let norm = self.normalized();
                    let new_norm = (norm + scroll_y * step).clamp(0.0, 1.0);
                    let new_value = self.denormalize(new_norm);

                    return (
                        canvas::event::Status::Captured,
                        Some(Message::SetEffectParam(
                            self.track_id,
                            self.effect_id,
                            self.param_index,
                            new_value,
                        )),
                    );
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
