//! Beat ruler: bar numbers, loop-region drag, seek clicks.

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig};
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;

/// Canvas widget that draws a beat-based ruler with bar.beat labels.
pub struct RulerWidget {
    pub playhead_beats: f64,
    pub bpm: f64,
    pub zoom_level: f32,
    pub grid: GridConfig,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
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

    fn visible_beats(&self, width: f32) -> f64 {
        self.geometry().visible_beats(width)
    }

    fn beat_to_x(&self, beat: f64, _width: f32) -> f32 {
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

        if self.bpm > 0.0 {
            let beats_per_bar = 4.0_f64;
            let visible = self.visible_beats(w);
            let start_beat = self.scroll_offset_beats.floor().max(0.0) as i64;
            let end_beat = (self.scroll_offset_beats + visible).ceil() as i64 + 1;

            // Adaptive label density based on zoom
            let ppb = self.pixels_per_beat();
            let pixels_per_bar = ppb * beats_per_bar as f32;

            // Adaptive: ensure ~60px minimum between labeled bars
            let bar_step: i64 = if pixels_per_bar >= 60.0 {
                1
            } else if pixels_per_bar >= 30.0 {
                2
            } else if pixels_per_bar >= 15.0 {
                4
            } else if pixels_per_bar >= 8.0 {
                8
            } else if pixels_per_bar >= 4.0 {
                16
            } else {
                32
            };

            for beat_i in start_beat..end_beat {
                let beat = beat_i as f64;
                let x = self.beat_to_x(beat, w);

                if x < -10.0 || x > w + 10.0 {
                    continue;
                }

                let bar_index = (beat / beats_per_bar).floor() as i64;
                let beat_in_bar = (beat % beats_per_bar) as usize;
                let is_bar = beat_in_bar == 0;

                if is_bar {
                    let show_label = (bar_index % bar_step) == 0;
                    let show_tick = show_label || pixels_per_bar >= 10.0;

                    if show_tick {
                        // Bar line (thick)
                        let tick = canvas::Path::line(
                            iced::Point::new(x, h * 0.3),
                            iced::Point::new(x, h),
                        );
                        frame.stroke(
                            &tick,
                            canvas::Stroke::default()
                                .with_color(theme::border())
                                .with_width(1.5),
                        );
                    }

                    if show_label {
                        // Bar label
                        if ppb < 40.0 {
                            // Low zoom: bar numbers only ("1", "2", "3")
                            let label = format!("{}", bar_index + 1);
                            frame.fill_text(canvas::Text {
                                content: label,
                                position: iced::Point::new(x + 4.0, 3.0),
                                color: theme::ruler_text(),
                                size: iced::Pixels(12.0),
                                ..Default::default()
                            });
                        } else {
                            // Medium/high zoom: bar.beat ("1.1", "2.1")
                            let label = format!("{}.1", bar_index + 1);
                            frame.fill_text(canvas::Text {
                                content: label,
                                position: iced::Point::new(x + 4.0, 3.0),
                                color: theme::ruler_text(),
                                size: iced::Pixels(12.0),
                                ..Default::default()
                            });
                        }
                    }
                } else if ppb >= 40.0 {
                    // Beat ticks only at medium+ zoom
                    let tick =
                        canvas::Path::line(iced::Point::new(x, h * 0.65), iced::Point::new(x, h));
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::divider())
                            .with_width(0.5),
                    );

                    // Beat labels only at high zoom (≥80 ppb)
                    if ppb >= 80.0 {
                        let label = format!("{}.{}", bar_index + 1, beat_in_bar + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 2.0, 6.0),
                            color: theme::text_muted(),
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Sub-beat ticks at very high zoom (>120 ppb)
                if ppb > 120.0 {
                    for sub in 1..4 {
                        let sub_beat = beat + sub as f64 * 0.25;
                        let sub_x = self.beat_to_x(sub_beat, w);
                        if sub_x > 0.0 && sub_x < w {
                            let sub_tick = canvas::Path::line(
                                iced::Point::new(sub_x, h * 0.8),
                                iced::Point::new(sub_x, h),
                            );
                            frame.stroke(
                                &sub_tick,
                                canvas::Stroke::default()
                                    .with_color(theme::divider())
                                    .with_width(0.3),
                            );
                        }
                    }
                }
            }

            // Ruler bottom border
            let bottom_border =
                canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
            frame.stroke(
                &bottom_border,
                canvas::Stroke::default()
                    .with_color(theme::border())
                    .with_width(1.0),
            );
        }

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
                    let (dx, dy) = match delta {
                        iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                        iced::mouse::ScrollDelta::Pixels { x, y } => (x / 20.0, y / 20.0),
                    };
                    // Horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(-dx as f64 * 2.0))),
                        );
                    }
                    // Shift+scroll for zoom
                    if state.shift_held && dy.abs() > 0.0 {
                        if dy > 0.0 {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::View(ViewMsg::ZoomIn)),
                            );
                        } else {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::View(ViewMsg::ZoomOut)),
                            );
                        }
                    }
                    // Plain scroll for horizontal panning
                    if dy.abs() > 0.0 {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(dy as f64 * 2.0))),
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

// ── TrackClipCanvas ──
