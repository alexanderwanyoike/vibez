use std::time::{Duration, Instant};

use iced::keyboard;
use iced::mouse;
use iced::widget::{canvas, column, text};
use iced::{Color, Element, Length, Rectangle, Renderer, Theme};

use crate::message::{DrumPadParam, Message};
use crate::state::UndoGestureId;
use crate::theme;
use crate::widgets::double_click::DoubleClick;
use crate::widgets::drag::ValueDrag;
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

/// What a parameter knob controls: an effect slot or the track's
/// native instrument. Determines which message value changes emit.
#[derive(Clone, Copy)]
pub enum KnobTarget {
    Effect(EffectId),
    Instrument,
    DrumPad {
        pad_index: usize,
        param: DrumPadParam,
    },
    /// Post-fader send amount into a bus.
    Send {
        bus_id: TrackId,
    },
}

/// Generalized rotary knob widget for device parameters with
/// arbitrary min/max (effects and native instruments).
pub struct EffectKnobWidget {
    pub track_id: TrackId,
    pub target: KnobTarget,
    pub param_index: usize,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub arc_color: Color,
}

impl EffectKnobWidget {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        track_id: TrackId,
        effect_id: EffectId,
        param_index: usize,
        value: f32,
        min: f32,
        max: f32,
        default: f32,
        arc_color: Color,
    ) -> Self {
        Self {
            track_id,
            target: KnobTarget::Effect(effect_id),
            param_index,
            value,
            min,
            max,
            default,
            arc_color,
        }
    }

    /// Knob bound to the track's native instrument parameter.
    #[allow(clippy::too_many_arguments)]
    pub fn for_instrument(
        track_id: TrackId,
        param_index: usize,
        value: f32,
        min: f32,
        max: f32,
        default: f32,
        arc_color: Color,
    ) -> Self {
        Self {
            track_id,
            target: KnobTarget::Instrument,
            param_index,
            value,
            min,
            max,
            default,
            arc_color,
        }
    }

    /// Knob bound to a track's send amount into a bus (0..1, off by
    /// default).
    pub fn for_send(track_id: TrackId, bus_id: TrackId, value: f32, arc_color: Color) -> Self {
        Self {
            track_id,
            target: KnobTarget::Send { bus_id },
            param_index: 0,
            value,
            min: 0.0,
            max: 1.0,
            default: 0.0,
            arc_color,
        }
    }

    /// Knob bound to a drum rack pad parameter.
    #[allow(clippy::too_many_arguments)]
    pub fn for_drum_pad(
        track_id: TrackId,
        pad_index: usize,
        param: DrumPadParam,
        value: f32,
        min: f32,
        max: f32,
        default: f32,
        arc_color: Color,
    ) -> Self {
        Self {
            track_id,
            target: KnobTarget::DrumPad { pad_index, param },
            param_index: 0,
            value,
            min,
            max,
            default,
            arc_color,
        }
    }

    fn set_value_message(&self, value: f32) -> Message {
        match self.target {
            KnobTarget::Effect(effect_id) => {
                Message::set_effect_param(self.track_id, effect_id, self.param_index, value)
            }
            KnobTarget::Instrument => {
                Message::set_instrument_param(self.track_id, self.param_index, value)
            }
            KnobTarget::DrumPad { pad_index, param } => {
                Message::Devices(crate::domains::devices::DevicesMsg::SetDrumPadParam {
                    track_id: self.track_id,
                    pad_index,
                    param,
                    value,
                })
            }
            KnobTarget::Send { bus_id } => Message::set_send(self.track_id, bus_id, value),
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

/// Standard width of a knob+label+value column in device cards.
pub const PARAM_COLUMN_WIDTH: f32 = 56.0;
/// Standard knob canvas size in device cards.
pub const KNOB_SIZE: f32 = 36.0;

/// The one true knob column: knob, name, value. Every device card
/// (effects and instruments) uses this so density, typography, and
/// spacing stay consistent across the whole device chain.
pub fn param_column<'a>(
    knob: EffectKnobWidget,
    label: String,
    value_text: String,
) -> Element<'a, Message> {
    let knob_canvas: Element<'a, Message> = canvas(knob)
        .width(Length::Fixed(KNOB_SIZE))
        .height(Length::Fixed(KNOB_SIZE))
        .into();
    column![
        knob_canvas,
        text(label).size(9).color(theme::text_dim()),
        text(value_text).size(9).color(theme::text()),
    ]
    .spacing(2)
    .width(Length::Fixed(PARAM_COLUMN_WIDTH))
    .align_x(iced::Alignment::Center)
    .into()
}

/// Musical value formatting shared by every device card: note names
/// for pitch params, kHz above 1000 Hz, ms below one second.
pub fn format_value(value: f32, unit: &str) -> String {
    match unit {
        "note" => crate::widgets::piano_roll::pitch_name(value.round().clamp(0.0, 127.0) as u8),
        "Hz" if value >= 1000.0 => format!("{:.1} kHz", value / 1000.0),
        "Hz" => format!("{value:.0} Hz"),
        "s" if value < 1.0 => format!("{:.0} ms", value * 1000.0),
        "s" => format!("{value:.2} s"),
        "" => format!("{value:.2}"),
        other => format!("{value:.1} {other}"),
    }
}

/// State for mouse interaction.
#[derive(Debug, Default)]
pub struct EffectKnobState {
    drag: ValueDrag,
    undo_gesture: Option<UndoGestureId>,
    shift_held: bool,
    double_click: DoubleClick,
}

impl canvas::Program<Message> for EffectKnobWidget {
    type State = EffectKnobState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let center = iced::Point::new(w / 2.0, h / 2.0);
        let radius = (w.min(h) / 2.0 - 3.0).max(6.0);
        let engaged = state.drag.is_active() || cursor.is_over(bounds);

        // Geometry: the value arc rides outside the body with clear
        // air between every element; overlapping strokes at nearly
        // identical radii were the source of the shimmer artifacts.
        let arc_radius = radius - 1.25;
        let body_radius = radius - 4.5;
        let norm = self.normalized();
        let value_angle = ARC_START + norm * (ARC_END - ARC_START);

        // Track arc first, then the knob body on top. Round caps keep
        // the arc ends crisp.
        let round = canvas::Stroke {
            line_cap: canvas::LineCap::Round,
            ..canvas::Stroke::default()
        };
        let bg_arc = build_arc(center, arc_radius, ARC_START, ARC_END);
        frame.stroke(
            &bg_arc,
            round.with_color(theme::knob_track()).with_width(2.5),
        );

        if norm > 0.005 {
            let value_arc = build_arc(center, arc_radius, ARC_START, value_angle);
            let arc_color = if engaged {
                Color {
                    r: (self.arc_color.r * 1.2).min(1.0),
                    g: (self.arc_color.g * 1.2).min(1.0),
                    b: (self.arc_color.b * 1.2).min(1.0),
                    a: 1.0,
                }
            } else {
                self.arc_color
            };
            frame.stroke(&value_arc, round.with_color(arc_color).with_width(2.5));
        }

        // Knob body: filled disc, subtly lighter while engaged.
        let body = canvas::Path::circle(center, body_radius);
        frame.fill(
            &body,
            if engaged {
                theme::knob_body_engaged()
            } else {
                theme::knob_body()
            },
        );

        // Pointer: from inside the body to its edge, rounded, never
        // touching the arc.
        let pointer_inner = body_radius * 0.35;
        let pointer_outer = body_radius - 1.0;
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
            round
                .with_color(if engaged {
                    theme::text()
                } else {
                    theme::text_dim()
                })
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
            // Track modifier keys
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                return (canvas::event::Status::Ignored, None);
            }

            // Click: start drag or double-click to reset
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    // Double-click detection: reset to default
                    if let Some(pos) = cursor.position() {
                        if state.double_click.press(
                            Instant::now(),
                            pos,
                            Duration::from_millis(DOUBLE_CLICK_MS),
                            None,
                        ) {
                            state.double_click.clear();
                            return (
                                canvas::event::Status::Captured,
                                Some(self.set_value_message(self.default)),
                            );
                        }
                    }

                    if state.drag.grab(cursor, bounds, self.normalized()) {
                        state.undo_gesture = Some(UndoGestureId::new());
                    }
                    return (canvas::event::Status::Captured, None);
                }
            }

            // Release
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.release() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }

            // Drag: vertical movement adjusts value (up = positive).
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let sensitivity = if state.shift_held {
                    BASE_SENSITIVITY / FINE_DIVISOR
                } else {
                    BASE_SENSITIVITY
                };
                if let Some(norm) = state.drag.drag_to(cursor, 0.0, -sensitivity, 0.0..=1.0) {
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            self.set_value_message(self.denormalize(norm))
                                .in_undo_gesture(state.undo_gesture.unwrap()),
                        ),
                    );
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

                let norm = self.normalized();
                let new_norm = (norm + scroll_y * step).clamp(0.0, 1.0);
                let new_value = self.denormalize(new_norm);

                return (
                    canvas::event::Status::Captured,
                    Some(self.set_value_message(new_value)),
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
