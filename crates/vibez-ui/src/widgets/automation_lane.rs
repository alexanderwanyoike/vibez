//! Automation lane canvas: breakpoints over time, aligned with the
//! arrangement timeline (same pixels-per-beat and scroll offset).
//!
//! Interactions:
//! - double-click adds a point; drag moves one (ghost while
//!   dragging, commit on release); right-click or Delete removes
//! - alt-drag a segment bends its curve (alt-double-click resets it
//!   straight); the cursor switches to a vertical-resize arrow
//! - ctrl-drag sweeps a beat range and erases every point inside
//!   (crosshair cursor)
//!
//! The dotted line is the parameter's current (un-automated) value;
//! min/max labels give the scale.

use std::time::Instant;

use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::automation::{shape, AutomationPoint};
use vibez_core::id::{LaneId, TrackId};

use crate::domains::automation::AutomationMsg;
use crate::message::Message;
use crate::state::SnapGrid;
use crate::theme as th;

pub const LANE_HEIGHT: f32 = 56.0;
const HANDLE_RADIUS: f32 = 5.0;
const HIT_RADIUS: f32 = 9.0;
const SEGMENT_HIT: f32 = 10.0;
const PAD_TOP: f32 = 8.0;
const PAD_BOTTOM: f32 = 8.0;
/// Vertical drag distance for a full curve sweep.
const CURVE_DRAG_RANGE: f32 = 90.0;

pub struct AutomationLaneWidget {
    pub track_id: TrackId,
    pub lane_id: LaneId,
    pub points: Vec<AutomationPoint>,
    pub color: Color,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    /// Grid for beat snapping (hold shift to bypass).
    pub snap: SnapGrid,
    pub selected: Option<usize>,
    /// The parameter's current un-automated value (normalized), for
    /// the dotted reference line.
    pub reference: Option<f32>,
    pub min_label: String,
    pub max_label: String,
    pub ref_label: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaneInteraction {
    /// Point index being dragged and its current ghost position.
    drag: Option<(usize, f64, f32)>,
    /// (segment index, original curve, press y, ghost curve).
    curve_drag: Option<(usize, f32, f32, f32)>,
    /// (start beat, current beat) of a ctrl-drag erase sweep.
    erase_drag: Option<(f64, f64)>,
    /// Last left press, for double-click detection.
    last_click: Option<(Instant, Point)>,
    alt: bool,
    ctrl: bool,
    shift: bool,
}

impl AutomationLaneWidget {
    fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    fn x_to_beat(&self, x: f32) -> f64 {
        (x as f64 / self.pixels_per_beat() as f64 + self.scroll_offset_beats).max(0.0)
    }

    /// Cursor x to beat, snapped to the grid unless shift is held.
    fn x_to_snapped_beat(&self, x: f32, state: &LaneInteraction) -> f64 {
        let beat = self.x_to_beat(x);
        if state.shift {
            beat
        } else {
            self.snap.snap_beat(beat).max(0.0)
        }
    }

    fn value_to_y(&self, value: f32, height: f32) -> f32 {
        let usable = height - PAD_TOP - PAD_BOTTOM;
        PAD_TOP + (1.0 - value.clamp(0.0, 1.0)) * usable
    }

    fn y_to_value(&self, y: f32, height: f32) -> f32 {
        let usable = height - PAD_TOP - PAD_BOTTOM;
        (1.0 - (y - PAD_TOP) / usable).clamp(0.0, 1.0)
    }

    fn hit_point(&self, pos: Point, height: f32) -> Option<usize> {
        self.points.iter().position(|p| {
            let px = self.beat_to_x(p.beat);
            let py = self.value_to_y(p.value, height);
            (px - pos.x).powi(2) + (py - pos.y).powi(2) <= HIT_RADIUS * HIT_RADIUS
        })
    }

    /// The segment (index of its left point) under the cursor, if the
    /// cursor is horizontally inside it and vertically near the curve.
    fn hit_segment(&self, pos: Point, height: f32) -> Option<usize> {
        for i in 0..self.points.len().saturating_sub(1) {
            let a = self.points[i];
            let b = self.points[i + 1];
            let x0 = self.beat_to_x(a.beat);
            let x1 = self.beat_to_x(b.beat);
            if pos.x < x0 || pos.x > x1 || x1 - x0 < 2.0 {
                continue;
            }
            let t = (pos.x - x0) / (x1 - x0);
            let value = a.value + (b.value - a.value) * shape(t, a.curve);
            let y = self.value_to_y(value, height);
            if (y - pos.y).abs() <= SEGMENT_HIT {
                return Some(i);
            }
        }
        None
    }

    /// Ghost curve from a vertical drag, oriented so dragging down
    /// always bulges the segment downward.
    fn curve_from_drag(&self, index: usize, orig: f32, press_y: f32, y: f32) -> f32 {
        let rising = self
            .points
            .get(index + 1)
            .zip(self.points.get(index))
            .map(|(b, a)| b.value >= a.value)
            .unwrap_or(true);
        let dir = if rising { 1.0 } else { -1.0 };
        (orig + (y - press_y) / CURVE_DRAG_RANGE * dir).clamp(-1.0, 1.0)
    }
}

impl canvas::Program<Message> for AutomationLaneWidget {
    type State = LaneInteraction;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let h = bounds.height;

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Color::from_rgba(0.0, 0.0, 0.0, 0.18),
        );

        // Dotted reference: where the knob sits without automation.
        if let Some(reference) = self.reference {
            let y = self.value_to_y(reference, h);
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y), Point::new(bounds.width, y)),
                canvas::Stroke {
                    line_dash: canvas::LineDash {
                        segments: &[3.0, 5.0],
                        offset: 0,
                    },
                    ..canvas::Stroke::default()
                        .with_color(Color {
                            a: 0.35,
                            ..th::text()
                        })
                        .with_width(1.0)
                },
            );
        }

        // Scale labels: max top-left, min bottom-left, reference value
        // against the right edge on its line.
        let label = |content: &str, position: Point, bottom: bool| canvas::Text {
            content: content.to_string(),
            position,
            color: Color {
                a: 0.55,
                ..th::text()
            },
            size: iced::Pixels(9.0),
            vertical_alignment: if bottom {
                iced::alignment::Vertical::Bottom
            } else {
                iced::alignment::Vertical::Top
            },
            ..canvas::Text::default()
        };
        frame.fill_text(label(&self.max_label, Point::new(6.0, 2.0), false));
        frame.fill_text(label(&self.min_label, Point::new(6.0, h - 2.0), true));
        if let Some(reference) = self.reference {
            if !self.ref_label.is_empty() {
                let y = self.value_to_y(reference, h);
                let mut t = label(
                    &self.ref_label,
                    Point::new(bounds.width - 6.0, y - 3.0),
                    true,
                );
                t.horizontal_alignment = iced::alignment::Horizontal::Right;
                frame.fill_text(t);
            }
        }

        // Working copy with drag ghosts applied.
        let mut pts: Vec<AutomationPoint> = self.points.clone();
        if let Some((idx, beat, value)) = state.drag {
            if idx < pts.len() {
                let curve = pts[idx].curve;
                pts.remove(idx);
                let insert_at = pts.partition_point(|p| p.beat < beat);
                pts.insert(insert_at, AutomationPoint { beat, value, curve });
            }
        }
        if let Some((idx, _, _, ghost)) = state.curve_drag {
            if idx < pts.len() {
                pts[idx].curve = ghost;
            }
        }

        let curve_color = self.color;
        if !pts.is_empty() {
            let mut path = canvas::path::Builder::new();
            let first_y = self.value_to_y(pts[0].value, h);
            path.move_to(Point::new(0.0, first_y));
            path.line_to(Point::new(self.beat_to_x(pts[0].beat), first_y));
            for i in 0..pts.len() - 1 {
                let a = pts[i];
                let b = pts[i + 1];
                let x0 = self.beat_to_x(a.beat);
                let x1 = self.beat_to_x(b.beat);
                if a.curve == 0.0 {
                    path.line_to(Point::new(x1, self.value_to_y(b.value, h)));
                } else {
                    const STEPS: usize = 24;
                    for step in 1..=STEPS {
                        let t = step as f32 / STEPS as f32;
                        let v = a.value + (b.value - a.value) * shape(t, a.curve);
                        path.line_to(Point::new(x0 + (x1 - x0) * t, self.value_to_y(v, h)));
                    }
                }
            }
            let last = pts[pts.len() - 1];
            let last_y = self.value_to_y(last.value, h);
            path.line_to(Point::new(bounds.width, last_y));
            frame.stroke(
                &path.build(),
                canvas::Stroke::default()
                    .with_color(curve_color)
                    .with_width(2.0),
            );

            for (i, p) in pts.iter().enumerate() {
                let center = Point::new(self.beat_to_x(p.beat), self.value_to_y(p.value, h));
                let selected = state.drag.map(|(d, _, _)| d == i).unwrap_or(false)
                    || (state.drag.is_none() && self.selected == Some(i));
                let handle = canvas::Path::circle(center, HANDLE_RADIUS);
                if selected {
                    frame.fill(&handle, th::accent());
                } else {
                    frame.fill(&handle, th::bg_dark());
                    frame.stroke(
                        &handle,
                        canvas::Stroke::default()
                            .with_color(curve_color)
                            .with_width(2.0),
                    );
                }
            }
        }

        // Erase sweep overlay.
        if let Some((b0, b1)) = state.erase_drag {
            let (lo, hi) = if b0 <= b1 { (b0, b1) } else { (b1, b0) };
            let x0 = self.beat_to_x(lo).max(0.0);
            let x1 = self.beat_to_x(hi).min(bounds.width);
            if x1 > x0 {
                frame.fill_rectangle(
                    Point::new(x0, 0.0),
                    iced::Size::new(x1 - x0, h),
                    Color {
                        a: 0.18,
                        ..th::danger()
                    },
                );
            }
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        // Modifier tracking works regardless of cursor position.
        if let canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) = event {
            state.alt = m.alt();
            state.ctrl = m.control();
            state.shift = m.shift();
            return (canvas::event::Status::Ignored, None);
        }

        let commit_release = |state: &mut Self::State| -> Option<Message> {
            if let Some((index, beat, value)) = state.drag.take() {
                return Some(Message::Automation(AutomationMsg::MovePoint {
                    track_id: self.track_id,
                    lane_id: self.lane_id,
                    index,
                    beat,
                    value,
                }));
            }
            if let Some((index, orig, _, ghost)) = state.curve_drag.take() {
                if (ghost - orig).abs() > 0.001 {
                    return Some(Message::Automation(AutomationMsg::SetCurve {
                        track_id: self.track_id,
                        lane_id: self.lane_id,
                        index,
                        curve: ghost,
                    }));
                }
                return None;
            }
            if let Some((b0, b1)) = state.erase_drag.take() {
                if (b1 - b0).abs() > 0.01 {
                    return Some(Message::Automation(AutomationMsg::RemovePointsInRange {
                        track_id: self.track_id,
                        lane_id: self.lane_id,
                        start_beat: b0,
                        end_beat: b1,
                    }));
                }
            }
            None
        };

        let Some(pos) = cursor.position_in(bounds) else {
            if let canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event {
                if let Some(msg) = commit_release(state) {
                    return (canvas::event::Status::Captured, Some(msg));
                }
            }
            return (canvas::event::Status::Ignored, None);
        };
        let h = bounds.height;

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                // Ctrl: sweep-erase.
                if state.ctrl {
                    let beat = self.x_to_snapped_beat(pos.x, state);
                    state.erase_drag = Some((beat, beat));
                    state.last_click = None;
                    return (canvas::event::Status::Captured, None);
                }
                // Alt: bend a segment (alt-double-click resets it).
                if state.alt {
                    if let Some(index) = self.hit_segment(pos, h) {
                        let now = Instant::now();
                        if let Some((t, p)) = state.last_click {
                            let close = (p.x - pos.x).abs() < 6.0 && (p.y - pos.y).abs() < 6.0;
                            if close && now.duration_since(t).as_millis() < 400 {
                                state.last_click = None;
                                state.curve_drag = None;
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::Automation(AutomationMsg::SetCurve {
                                        track_id: self.track_id,
                                        lane_id: self.lane_id,
                                        index,
                                        curve: 0.0,
                                    })),
                                );
                            }
                        }
                        state.last_click = Some((now, pos));
                        let orig = self.points[index].curve;
                        state.curve_drag = Some((index, orig, pos.y, orig));
                        return (canvas::event::Status::Captured, None);
                    }
                    return (canvas::event::Status::Ignored, None);
                }
                if let Some(index) = self.hit_point(pos, h) {
                    let p = self.points[index];
                    state.drag = Some((index, p.beat, p.value));
                    state.last_click = None;
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Automation(AutomationMsg::SelectPoint {
                            track_id: self.track_id,
                            lane_id: self.lane_id,
                            index: Some(index),
                        })),
                    );
                }
                // Double-click on empty lane space adds a point.
                let now = Instant::now();
                if let Some((t, p)) = state.last_click {
                    let close = (p.x - pos.x).abs() < 6.0 && (p.y - pos.y).abs() < 6.0;
                    if close && now.duration_since(t).as_millis() < 400 {
                        state.last_click = None;
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Automation(AutomationMsg::AddPoint {
                                track_id: self.track_id,
                                lane_id: self.lane_id,
                                beat: self.x_to_snapped_beat(pos.x, state),
                                value: self.y_to_value(pos.y, h),
                            })),
                        );
                    }
                }
                state.last_click = Some((now, pos));
                (
                    canvas::event::Status::Captured,
                    Some(Message::Automation(AutomationMsg::SelectPoint {
                        track_id: self.track_id,
                        lane_id: self.lane_id,
                        index: None,
                    })),
                )
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((index, _, _)) = state.drag {
                    state.drag = Some((
                        index,
                        self.x_to_snapped_beat(pos.x, state),
                        self.y_to_value(pos.y, h),
                    ));
                    return (canvas::event::Status::Captured, None);
                }
                if let Some((index, orig, press_y, _)) = state.curve_drag {
                    let ghost = self.curve_from_drag(index, orig, press_y, pos.y);
                    state.curve_drag = Some((index, orig, press_y, ghost));
                    return (canvas::event::Status::Captured, None);
                }
                if let Some((b0, _)) = state.erase_drag {
                    state.erase_drag = Some((b0, self.x_to_snapped_beat(pos.x, state)));
                    return (canvas::event::Status::Captured, None);
                }
                (canvas::event::Status::Ignored, None)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                match commit_release(state) {
                    Some(msg) => (canvas::event::Status::Captured, Some(msg)),
                    None => (canvas::event::Status::Ignored, None),
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(index) = self.hit_point(pos, h) {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Automation(AutomationMsg::RemovePoint {
                            track_id: self.track_id,
                            lane_id: self.lane_id,
                            index,
                        })),
                    );
                }
                (canvas::event::Status::Ignored, None)
            }
            _ => (canvas::event::Status::Ignored, None),
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if state.curve_drag.is_some() {
            return mouse::Interaction::ResizingVertically;
        }
        if state.erase_drag.is_some() {
            return mouse::Interaction::Crosshair;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            if state.ctrl {
                return mouse::Interaction::Crosshair;
            }
            if state.alt {
                if self.hit_segment(pos, bounds.height).is_some() {
                    return mouse::Interaction::ResizingVertically;
                }
                return mouse::Interaction::default();
            }
            if self.hit_point(pos, bounds.height).is_some() {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::default()
    }
}
