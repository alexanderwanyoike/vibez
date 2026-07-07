//! Automation lane canvas: breakpoints over time, aligned with the
//! arrangement timeline (same pixels-per-beat and scroll offset).
//!
//! Interactions: double-click adds a point, drag moves one (the
//! move commits on release; a ghost renders during the drag), click
//! selects, Delete removes via the global delete-key priority.

use std::time::Instant;

use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::automation::AutomationPoint;
use vibez_core::id::{LaneId, TrackId};

use crate::domains::automation::AutomationMsg;
use crate::message::Message;
use crate::theme as th;

pub const LANE_HEIGHT: f32 = 56.0;
const HANDLE_RADIUS: f32 = 5.0;
const HIT_RADIUS: f32 = 9.0;
const PAD_TOP: f32 = 8.0;
const PAD_BOTTOM: f32 = 8.0;

pub struct AutomationLaneWidget {
    pub track_id: TrackId,
    pub lane_id: LaneId,
    pub points: Vec<AutomationPoint>,
    pub color: Color,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub selected: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaneInteraction {
    /// Point index being dragged and its current ghost position.
    drag: Option<(usize, f64, f32)>,
    /// Last left press, for double-click detection.
    last_click: Option<(Instant, Point)>,
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

        // Ground and midline.
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Color::from_rgba(0.0, 0.0, 0.0, 0.18),
        );
        let mid = self.value_to_y(0.5, h);
        frame.stroke(
            &canvas::Path::line(Point::new(0.0, mid), Point::new(bounds.width, mid)),
            canvas::Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))
                .with_width(1.0),
        );

        // The curve, with the dragged point replaced by its ghost.
        let mut pts: Vec<AutomationPoint> = self.points.clone();
        if let Some((idx, beat, value)) = state.drag {
            if idx < pts.len() {
                pts.remove(idx);
                let insert_at = pts.partition_point(|p| p.beat < beat);
                pts.insert(insert_at, AutomationPoint { beat, value });
            }
        }

        let curve_color = self.color;
        if !pts.is_empty() {
            let mut path = canvas::path::Builder::new();
            let first_y = self.value_to_y(pts[0].value, h);
            path.move_to(Point::new(0.0, first_y));
            path.line_to(Point::new(self.beat_to_x(pts[0].beat), first_y));
            for p in &pts {
                path.line_to(Point::new(
                    self.beat_to_x(p.beat),
                    self.value_to_y(p.value, h),
                ));
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
                    frame.fill(&handle, th::ACCENT);
                } else {
                    frame.fill(&handle, Color::from_rgba(0.09, 0.09, 0.1, 1.0));
                    frame.stroke(
                        &handle,
                        canvas::Stroke::default()
                            .with_color(curve_color)
                            .with_width(2.0),
                    );
                }
            }
        } else {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, self.value_to_y(1.0, h)),
                    Point::new(bounds.width, self.value_to_y(1.0, h)),
                ),
                canvas::Stroke::default()
                    .with_color(Color {
                        a: 0.35,
                        ..curve_color
                    })
                    .with_width(1.5),
            );
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
        let Some(pos) = cursor.position_in(bounds) else {
            // Commit an in-flight drag even if the release lands
            // outside the lane.
            if let canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event {
                if let Some((index, beat, value)) = state.drag.take() {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Automation(AutomationMsg::MovePoint {
                            track_id: self.track_id,
                            lane_id: self.lane_id,
                            index,
                            beat,
                            value,
                        })),
                    );
                }
            }
            return (canvas::event::Status::Ignored, None);
        };
        let h = bounds.height;

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
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
                                beat: self.x_to_beat(pos.x),
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
                    state.drag = Some((index, self.x_to_beat(pos.x), self.y_to_value(pos.y, h)));
                    return (canvas::event::Status::Captured, None);
                }
                (canvas::event::Status::Ignored, None)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some((index, beat, value)) = state.drag.take() {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Automation(AutomationMsg::MovePoint {
                            track_id: self.track_id,
                            lane_id: self.lane_id,
                            index,
                            beat,
                            value,
                        })),
                    );
                }
                (canvas::event::Status::Ignored, None)
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
        if let Some(pos) = cursor.position_in(bounds) {
            if self.hit_point(pos, bounds.height).is_some() {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::default()
    }
}
