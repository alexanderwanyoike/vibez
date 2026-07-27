//! Arrangement minimap: overview strip with viewport drag.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;

/// Per-track data for the minimap overview.
pub struct MinimapTrack {
    pub color: Color,
    /// (start_beat, duration_beats) for each clip on this track.
    pub clips: Vec<(f64, f64)>,
}

/// Bird's-eye arrangement overview widget (Ableton-style minimap).
pub struct ArrangementMinimap {
    pub total_beats: f64,
    pub scroll_offset_beats: f64,
    pub zoom_level: f32,
    pub playhead_beats: f64,
    pub bpm: f64,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub tracks: Vec<MinimapTrack>,
}

impl ArrangementMinimap {
    fn visible_beats(&self, canvas_width: f32) -> f64 {
        TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats)
            .visible_beats(canvas_width)
    }
}

/// Interaction state for the minimap.
#[derive(Debug, Default)]
pub struct MinimapInteractionState {
    drag: Option<MinimapDragAction>,
}

#[derive(Debug)]
enum MinimapDragAction {
    /// Dragging the viewport rectangle (panning).
    PanViewport { start_x: f32, start_scroll: f64 },
    /// Clicked outside viewport — continuously scroll to cursor.
    Seeking,
}

impl canvas::Program<Message> for ArrangementMinimap {
    type State = MinimapInteractionState;

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
            theme::bg_elevated(),
        );

        if self.total_beats <= 0.0 || self.tracks.is_empty() {
            return vec![frame.into_geometry()];
        }

        let overview = TimelineGeometry::fitted(self.total_beats, w, 0.0);
        let num_tracks = self.tracks.len();
        let track_h = (h / num_tracks as f32).max(2.0);

        // Draw clips for each track
        for (i, track) in self.tracks.iter().enumerate() {
            let y = i as f32 * track_h;
            let clip_color = theme::with_alpha(track.color, 0.6);
            for &(start, dur) in &track.clips {
                let cx = overview.beat_to_x(start);
                let cw = overview.width_for_beats(dur).max(1.0);
                if cx + cw < 0.0 || cx > w {
                    continue;
                }
                frame.fill_rectangle(
                    iced::Point::new(cx, y),
                    iced::Size::new(cw, track_h),
                    clip_color,
                );
            }
        }

        // Loop region overlay
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let lx1 = overview.beat_to_x(self.loop_start_beats);
            let lx2 = overview.beat_to_x(self.loop_end_beats);
            let fill_x = lx1.max(0.0);
            let fill_w = lx2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::accent(), 0.15),
                );
            }
        }

        // Viewport rectangle
        let visible = self.visible_beats(w);
        let vx_start = overview.beat_to_x(self.scroll_offset_beats);
        let vx_end = overview.beat_to_x(self.scroll_offset_beats + visible);
        let vx = vx_start.max(0.0);
        let vw = vx_end.min(w) - vx;
        if vw > 0.0 {
            // Subtle fill
            frame.fill_rectangle(
                iced::Point::new(vx, 0.0),
                iced::Size::new(vw, h),
                Color {
                    a: 0.05,
                    ..theme::text()
                },
            );
            // Orange border
            let rect_path = canvas::Path::new(|b| {
                b.move_to(iced::Point::new(vx, 0.5));
                b.line_to(iced::Point::new(vx + vw, 0.5));
                b.line_to(iced::Point::new(vx + vw, h - 0.5));
                b.line_to(iced::Point::new(vx, h - 0.5));
                b.close();
            });
            frame.stroke(
                &rect_path,
                canvas::Stroke::default()
                    .with_color(theme::accent())
                    .with_width(1.5),
            );
        }

        // Playhead line
        let ph_x = overview.beat_to_x(self.playhead_beats);
        if ph_x >= 0.0 && ph_x <= w {
            let playhead =
                canvas::Path::line(iced::Point::new(ph_x, 0.0), iced::Point::new(ph_x, h));
            frame.stroke(
                &playhead,
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(1.0),
            );
        }

        // Bottom border
        let border =
            canvas::Path::line(iced::Point::new(0.0, h - 0.5), iced::Point::new(w, h - 0.5));
        frame.stroke(
            &border,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if self.total_beats <= 0.0 {
            return (canvas::event::Status::Ignored, None);
        }

        let overview = TimelineGeometry::fitted(self.total_beats, bounds.width, 0.0);
        let visible = self.visible_beats(bounds.width);

        match event {
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let click_beat = overview.x_to_beat(pos.x);

                    // Check if click is inside the viewport rectangle
                    let vx_start = overview.beat_to_x(self.scroll_offset_beats);
                    let vx_end = overview.beat_to_x(self.scroll_offset_beats + visible);

                    if pos.x >= vx_start && pos.x <= vx_end {
                        // Start panning the viewport
                        state.drag = Some(MinimapDragAction::PanViewport {
                            start_x: pos.x,
                            start_scroll: self.scroll_offset_beats,
                        });
                        return (canvas::event::Status::Captured, None);
                    }

                    // Click outside viewport — jump to center that beat
                    let target = (click_beat - visible / 2.0).max(0.0);
                    let delta = target - self.scroll_offset_beats;
                    state.drag = Some(MinimapDragAction::Seeking);
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::View(ViewMsg::ScrollArrangement(delta))),
                    );
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        match drag {
                            MinimapDragAction::PanViewport {
                                start_x,
                                start_scroll,
                            } => {
                                let dx = local_x - start_x;
                                let delta_beats = overview.beats_for_width(dx);
                                let target = (start_scroll + delta_beats).max(0.0);
                                let delta = target - self.scroll_offset_beats;
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::View(ViewMsg::ScrollArrangement(delta))),
                                );
                            }
                            MinimapDragAction::Seeking => {
                                let click_beat = overview.x_to_beat(local_x);
                                let target = (click_beat - visible / 2.0).max(0.0);
                                let delta = target - self.scroll_offset_beats;
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::View(ViewMsg::ScrollArrangement(delta))),
                                );
                            }
                        }
                    }
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if state.drag.is_some() =>
            {
                state.drag = None;
                return (canvas::event::Status::Captured, None);
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
        if self.total_beats <= 0.0 {
            return mouse::Interaction::default();
        }

        if let Some(ref drag) = state.drag {
            return match drag {
                MinimapDragAction::PanViewport { .. } => mouse::Interaction::Grabbing,
                MinimapDragAction::Seeking => mouse::Interaction::Pointer,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            let overview = TimelineGeometry::fitted(self.total_beats, bounds.width, 0.0);
            let visible = self.visible_beats(bounds.width);
            let vx_start = overview.beat_to_x(self.scroll_offset_beats);
            let vx_end = overview.beat_to_x(self.scroll_offset_beats + visible);

            if pos.x >= vx_start && pos.x <= vx_end {
                return mouse::Interaction::Grab;
            }

            return mouse::Interaction::Pointer;
        }

        mouse::Interaction::default()
    }
}
