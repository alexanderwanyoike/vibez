//! Beat ruler: bar numbers, loop-region drag, seek clicks.

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig};
use crate::timeline_geometry::TimelineGeometry;

/// Canvas widget that draws a beat-based ruler with bar.beat labels.
pub struct RulerWidget {
    pub playhead_beats: f64,
    /// Arrange's play-start marker; Section rulers have none.
    pub play_start_beats: Option<f64>,
    pub bpm: f64,
    pub zoom_level: f32,
    pub grid: GridConfig,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
    /// Arrange edits its loop with a brace in a strip above the numbers;
    /// a Section loops over its whole length and has no strip.
    pub loop_brace: bool,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
}

/// Active drag action on the ruler.
#[derive(Debug, Clone)]
enum RulerDragAction {
    PendingSeek { beat: f64, start_x: f32 },
    RegionSelect { anchor_beat: f64 },
}

/// Interaction state for the ruler widget.
#[derive(Debug, Default)]
pub struct RulerInteractionState {
    drag: Option<RulerDragAction>,
    shift_held: bool,
}

impl RulerWidget {
    pub(super) fn geometry(&self) -> TimelineGeometry {
        TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats)
    }

    fn pixels_per_beat(&self) -> f32 {
        self.geometry().pixels_per_beat()
    }

    pub(crate) fn beat_to_x(&self, beat: f64, _width: f32) -> f32 {
        self.geometry().beat_to_x(beat)
    }

    fn snapped_beat(&self, beat: f64) -> f64 {
        self.grid.snap_beat(beat, self.pixels_per_beat())
    }
}

impl canvas::Program<Message> for RulerWidget {
    type State = RulerInteractionState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![self.paint(renderer, bounds.size())]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            // Left-click: PendingSeek (may become RegionSelect on drag)
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let beat = self.geometry().x_to_beat(pos.x);
                    state.drag = Some(RulerDragAction::PendingSeek {
                        beat,
                        start_x: pos.x,
                    });
                    return (canvas::event::Status::Captured, None);
                }
            }
            // Mouse move: transition PendingSeek→RegionSelect, or update region
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        let beat = self.geometry().x_to_beat(local_x);

                        // Auto-scroll when dragging near ruler edges
                        if matches!(drag, RulerDragAction::RegionSelect { .. }) {
                            let edge_zone = 50.0_f32;
                            if local_x > bounds.width - edge_zone {
                                let overshoot = ((local_x - (bounds.width - edge_zone))
                                    / edge_zone)
                                    .clamp(0.0, 3.0);
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::View(ViewMsg::ScrollArrangement(
                                        overshoot as f64 * 2.0,
                                    ))),
                                );
                            }
                            if local_x < edge_zone && self.scroll_offset_beats > 0.0 {
                                let overshoot = ((edge_zone - local_x) / edge_zone).clamp(0.0, 3.0);
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::View(ViewMsg::ScrollArrangement(
                                        -(overshoot as f64 * 2.0),
                                    ))),
                                );
                            }
                        }

                        match drag {
                            RulerDragAction::PendingSeek {
                                beat: anchor,
                                start_x,
                            } => {
                                let dx = (local_x - start_x).abs();
                                if dx > 4.0 {
                                    let anchor = self.snapped_beat(*anchor);
                                    let current = self.snapped_beat(beat);
                                    let start = anchor.min(current);
                                    let end = anchor.max(current);
                                    state.drag = Some(RulerDragAction::RegionSelect {
                                        anchor_beat: anchor,
                                    });
                                    if end > start {
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::Arrangement(
                                                ArrangementMsg::SetTimeSelection {
                                                    start_beats: start,
                                                    end_beats: end,
                                                    track_id: None,
                                                },
                                            )),
                                        );
                                    }
                                }
                            }
                            RulerDragAction::RegionSelect { anchor_beat } => {
                                let current = self.snapped_beat(beat);
                                let start = anchor_beat.min(current);
                                let end = anchor_beat.max(current);
                                if end > start {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(
                                            ArrangementMsg::SetTimeSelection {
                                                start_beats: start,
                                                end_beats: end,
                                                track_id: None,
                                            },
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // Mouse release
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if let Some(ref drag) = state.drag {
                    let msg = match drag {
                        RulerDragAction::PendingSeek { beat, .. } => {
                            // Short click → seek + clear selection
                            Some(Message::Transport(TransportMsg::SeekToBeat(*beat)))
                        }
                        RulerDragAction::RegionSelect { .. } => {
                            // Completed region select
                            Some(Message::set_time_selection_active(true))
                        }
                    };
                    state.drag = None;
                    return (canvas::event::Status::Captured, msg);
                }
            }
            // Right-click on ruler: show time selection context menu (or arrangement-empty)
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let screen_x = bounds.x + pos.x;
                    let screen_y = bounds.y + pos.y;

                    if self.time_selection_active
                        && self.selection_end_beats > self.selection_start_beats
                    {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ShowContextMenu {
                                x: screen_x,
                                y: screen_y,
                                target: ContextMenuTarget::TimeSelection {
                                    start_beats: self.selection_start_beats,
                                    end_beats: self.selection_end_beats,
                                    track_id: None,
                                },
                            })),
                        );
                    }

                    return (
                        canvas::event::Status::Captured,
                        Some(Message::View(ViewMsg::ShowContextMenu {
                            x: screen_x,
                            y: screen_y,
                            target: ContextMenuTarget::ArrangementEmpty,
                        })),
                    );
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let (dx, dy) = crate::timeline_geometry::wheel_delta_pixels(delta);
                    // Horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(
                                -self.geometry().beats_for_width(dx),
                            ))),
                        );
                    }
                    // Shift+scroll for zoom
                    if state.shift_held && dy.abs() > 0.0 {
                        let anchor_x = cursor
                            .position_in(bounds)
                            .map(|position| position.x)
                            .unwrap_or(bounds.width / 2.0);
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ZoomAround {
                                factor: crate::timeline_geometry::zoom_factor_from_pixels(dy),
                                anchor_x,
                            })),
                        );
                    }
                    // Plain scroll for horizontal panning
                    if dy.abs() > 0.0 {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(
                                self.geometry().beats_for_width(dy),
                            ))),
                        );
                    }
                }
            }
            // Track shift key state for zoom
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(ref drag) = state.drag {
            return match drag {
                RulerDragAction::RegionSelect { .. } => mouse::Interaction::Crosshair,
                RulerDragAction::PendingSeek { .. } => mouse::Interaction::Pointer,
            };
        }
        if cursor.is_over(bounds) {
            return mouse::Interaction::Pointer;
        }
        mouse::Interaction::default()
    }
}

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod tests;
