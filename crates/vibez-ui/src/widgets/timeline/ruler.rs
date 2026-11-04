//! Beat ruler: bar numbers, loop-region drag, seek clicks.

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use super::ruler_scale::{ruler_marks, RulerMark};
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig};
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;
use crate::typography;

const MID_TICK_HEIGHT: f32 = 7.0;
const MINOR_TICK_HEIGHT: f32 = 4.0;

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
    fn geometry(&self) -> TimelineGeometry {
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
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, h),
            theme::ruler_bg(),
        );

        draw_marks(&mut frame, self.geometry(), w, 0.0, h);

        // Loop region overlay
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let loop_x1 = self.beat_to_x(self.loop_start_beats, w);
            let loop_x2 = self.beat_to_x(self.loop_end_beats, w);

            let fill_x = loop_x1.max(0.0);
            let fill_w = loop_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::accent(), 0.15),
                );

                // REPEAT icon centered in the loop region
                let center_x = fill_x + fill_w / 2.0;
                frame.fill_text(canvas::Text {
                    content: crate::icons::REPEAT.to_string(),
                    position: iced::Point::new(center_x - 6.0, (h - 12.0) / 2.0),
                    color: theme::with_alpha(theme::accent(), 0.7),
                    size: iced::Pixels(12.0),
                    font: crate::icons::ICON_FONT,
                    ..Default::default()
                });
            }

            // Bracket lines at boundaries
            if loop_x1 >= 0.0 && loop_x1 <= w {
                let bracket = canvas::Path::line(
                    iced::Point::new(loop_x1, 0.0),
                    iced::Point::new(loop_x1, h),
                );
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(2.0),
                );
            }
            if loop_x2 >= 0.0 && loop_x2 <= w {
                let bracket = canvas::Path::line(
                    iced::Point::new(loop_x2, 0.0),
                    iced::Point::new(loop_x2, h),
                );
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(2.0),
                );
            }
        }

        // Selection region overlay (separate from loop)
        if self.time_selection_active && self.selection_end_beats > self.selection_start_beats {
            let sel_x1 = self.beat_to_x(self.selection_start_beats, w);
            let sel_x2 = self.beat_to_x(self.selection_end_beats, w);

            let fill_x = sel_x1.max(0.0);
            let fill_w = sel_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::accent(), 0.10),
                );
            }

            // Thinner bracket lines
            if sel_x1 >= 0.0 && sel_x1 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x1, 0.0), iced::Point::new(sel_x1, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(1.0),
                );
            }
            if sel_x2 >= 0.0 && sel_x2 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x2, 0.0), iced::Point::new(sel_x2, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(1.0),
                );
            }
        }

        if let Some(play_start) = self.play_start_beats {
            let x = self.beat_to_x(play_start, w);
            if (0.0..=w).contains(&x) {
                let marker = canvas::Path::new(|path| {
                    path.move_to(iced::Point::new(x - 5.0, 0.0));
                    path.line_to(iced::Point::new(x + 5.0, 0.0));
                    path.line_to(iced::Point::new(x, 7.0));
                    path.close();
                });
                frame.fill(&marker, theme::playhead());
            }
        }

        // Playhead
        let playhead_x = self.beat_to_x(self.playhead_beats, w);
        if playhead_x >= 0.0 && playhead_x <= w {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, h),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(2.0),
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

/// The number strip's labels and ticks between `top` and `bottom`.
fn draw_marks(
    frame: &mut canvas::Frame,
    geometry: TimelineGeometry,
    width: f32,
    top: f32,
    bottom: f32,
) {
    let tick = theme::ruler_tick();
    for mark in ruler_marks(geometry, width) {
        match mark {
            RulerMark::Label { x, text, bar } => {
                frame.fill_rectangle(
                    iced::Point::new(x, top),
                    iced::Size::new(1.0, bottom - top),
                    tick,
                );
                let (font, size, color) = if bar {
                    (typography::PERFORM_LABEL, 11.0, theme::text())
                } else {
                    (typography::PERFORM_DISPLAY, 10.5, theme::text_dim())
                };
                frame.fill_text(canvas::Text {
                    content: text,
                    position: iced::Point::new(x + 4.0, top + 4.0),
                    color,
                    size: iced::Pixels(size),
                    font,
                    ..Default::default()
                });
            }
            RulerMark::Mid { x } => frame.fill_rectangle(
                iced::Point::new(x, bottom - MID_TICK_HEIGHT),
                iced::Size::new(1.0, MID_TICK_HEIGHT),
                tick,
            ),
            RulerMark::Minor { x } => frame.fill_rectangle(
                iced::Point::new(x, bottom - MINOR_TICK_HEIGHT),
                iced::Size::new(1.0, MINOR_TICK_HEIGHT),
                tick,
            ),
        }
    }
    frame.fill_rectangle(
        iced::Point::new(0.0, bottom - 1.0),
        iced::Size::new(width, 1.0),
        theme::border(),
    );
}

// ── TrackClipCanvas ──

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod tests;
