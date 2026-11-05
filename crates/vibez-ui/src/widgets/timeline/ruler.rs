//! The beat ruler's pointer gestures: loop brace drags, seek clicks, zoom
//! drags and wheel navigation.

use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas;
use iced::{Point, Rectangle, Renderer, Theme};

use super::ruler_drag::{brace_grab, BraceDrag};
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig, UndoGestureId};
use crate::timeline_geometry::{TimelineGeometry, TimelineViewport};
use crate::timeline_snap::{SnapContext, SnapTargets};
use crate::widgets::clip_loop_markers::LoopMarker;
use crate::widgets::double_click::DoubleClick;
use crate::widgets::drag_phase::{DragPhase, CLICK_SLOP_PX};

/// Height of the Arrange ruler: the loop strip over the number strip.
pub const ARRANGE_RULER_HEIGHT: f32 = 36.0;
pub(super) const LOOP_STRIP_HEIGHT: f32 = 13.0;
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);

/// Canvas widget that draws a beat-based ruler with bar.beat labels.
pub struct RulerWidget {
    pub playhead_beats: f64,
    /// Arrange's play-start marker; Section rulers have none.
    pub play_start_beats: Option<f64>,
    pub zoom_level: f32,
    pub grid: GridConfig,
    pub scroll_offset_beats: f64,
    /// Arrange edits its loop with a brace in a strip above the numbers;
    /// a Section loops over its whole length and has no strip.
    pub loop_brace: bool,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
    /// Clip edges and markers a seek click locks onto.
    pub snap_targets: std::sync::Arc<SnapTargets>,
}

/// Active drag action on the ruler.
#[derive(Debug, Clone)]
enum RulerDragAction {
    /// A press on the numbers sets the play start when released as a click
    /// and zooms and pans once dragged.
    Numbers {
        beat: f64,
        start: Point,
        view: TimelineViewport,
        phase: DragPhase,
    },
    Brace {
        drag: BraceDrag,
        gesture: UndoGestureId,
    },
}

/// Interaction state for the ruler widget.
#[derive(Debug, Default)]
pub struct RulerInteractionState {
    drag: Option<RulerDragAction>,
    double_click: DoubleClick,
    shift_held: bool,
    /// Bypasses all snapping, as Alt does in Ableton.
    alt_held: bool,
}

impl RulerWidget {
    pub(super) fn geometry(&self) -> TimelineGeometry {
        TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats)
    }

    fn pixels_per_beat(&self) -> f32 {
        self.geometry().pixels_per_beat()
    }

    /// `free` bypasses all snapping, as holding Alt does.
    fn snap_context(&self, free: bool) -> SnapContext {
        self.grid.snap_context(self.pixels_per_beat(), free)
    }

    /// Where the number strip starts, below the loop strip if there is one.
    pub(super) fn numbers_top(&self) -> f32 {
        if self.loop_brace {
            LOOP_STRIP_HEIGHT
        } else {
            0.0
        }
    }

    fn brace_grab_at(&self, x: f32) -> Option<LoopMarker> {
        let geometry = self.geometry();
        brace_grab(
            x,
            geometry.beat_to_x(self.loop_start_beats),
            geometry.beat_to_x(self.loop_end_beats),
        )
    }

    /// The loop edit for the pointer at `beat`, if it changes the loop.
    fn brace_edit(
        &self,
        drag: BraceDrag,
        gesture: UndoGestureId,
        beat: f64,
        free: bool,
    ) -> Option<Message> {
        let context = self.snap_context(free);
        let (start_beats, end_beats) = drag.loop_at(beat, &self.snap_targets, context)?;
        ((start_beats, end_beats) != (self.loop_start_beats, self.loop_end_beats)).then(|| {
            Message::Transport(TransportMsg::SetArrangementLoopRegion {
                start_beats,
                end_beats,
            })
            .in_undo_gesture(gesture)
        })
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
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let beat = self.geometry().x_to_beat(pos.x);
                    if pos.y < self.numbers_top() {
                        state.drag = Some(RulerDragAction::Brace {
                            drag: BraceDrag {
                                grab: self.brace_grab_at(pos.x),
                                original: (self.loop_start_beats, self.loop_end_beats),
                                pressed_beat: beat,
                            },
                            gesture: UndoGestureId::new(),
                        });
                        return (canvas::event::Status::Captured, None);
                    }
                    if state.double_click.press(
                        Instant::now(),
                        pos,
                        DOUBLE_CLICK_WINDOW,
                        Some(CLICK_SLOP_PX),
                    ) {
                        state.double_click.clear();
                        state.drag = None;
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ZoomToFit)),
                        );
                    }
                    state.drag = Some(RulerDragAction::Numbers {
                        beat,
                        start: pos,
                        view: TimelineViewport::new(self.zoom_level, self.scroll_offset_beats),
                        phase: DragPhase::Pending,
                    });
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let (Some(drag), Some(pos)) = (state.drag.clone(), cursor.position()) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let local = Point::new(pos.x - bounds.x, pos.y - bounds.y);
                let message = match drag {
                    RulerDragAction::Brace { drag, gesture } => self.brace_edit(
                        drag,
                        gesture,
                        self.geometry().x_to_beat(local.x),
                        state.alt_held,
                    ),
                    RulerDragAction::Numbers {
                        beat,
                        start,
                        view,
                        mut phase,
                    } => {
                        if !phase.follow(start, local) {
                            return (canvas::event::Status::Captured, None);
                        }
                        state.double_click.clear();
                        state.drag = Some(RulerDragAction::Numbers {
                            beat,
                            start,
                            view,
                            phase,
                        });
                        Some(zoom_drag(view, beat, start, local))
                    }
                };
                return (canvas::event::Status::Captured, message);
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if let Some(drag) = state.drag.take() {
                    let msg = match drag {
                        RulerDragAction::Numbers {
                            beat,
                            phase: DragPhase::Pending,
                            ..
                        } => {
                            let context = self.snap_context(state.alt_held);
                            let beat = self.snap_targets.click(beat, context).beat;
                            Some(Message::Transport(TransportMsg::SeekToBeat(beat)))
                        }
                        RulerDragAction::Numbers { .. } | RulerDragAction::Brace { .. } => None,
                    };
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
                state.alt_held = modifiers.alt();
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
                RulerDragAction::Numbers { .. } => mouse::Interaction::ZoomIn,
                RulerDragAction::Brace { drag, .. } => brace_interaction(drag.grab, true),
            };
        }
        match cursor.position_in(bounds) {
            Some(pos) if pos.y < self.numbers_top() => {
                brace_interaction(self.brace_grab_at(pos.x), false)
            }
            Some(_) => mouse::Interaction::ZoomIn,
            None => mouse::Interaction::default(),
        }
    }
}

/// The ruler drag message for the pointer at `at`, from a press at `start`.
fn zoom_drag(view: TimelineViewport, grabbed_beat: f64, start: Point, at: Point) -> Message {
    Message::View(ViewMsg::SetViewport(view.ruler_dragged(
        grabbed_beat,
        at.x,
        at.y - start.y,
    )))
}

fn brace_interaction(grab: Option<LoopMarker>, dragging: bool) -> mouse::Interaction {
    grab.map_or(mouse::Interaction::Crosshair, |marker| {
        marker.interaction(dragging)
    })
}

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod tests;
