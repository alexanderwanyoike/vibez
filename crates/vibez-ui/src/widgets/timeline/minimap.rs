//! Arrangement minimap: overview strip with viewport drag.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::theme;
use crate::timeline_geometry::{TimelineGeometry, TimelineViewport};
use crate::widgets::clip_loop_markers::{loop_with_end, loop_with_start, nearest_end, LoopMarker};

use super::loop_tint::LoopSpan;

/// The mockup's overview height, kept below the ruler's so the overview
/// never outweighs it.
pub const OVERVIEW_HEIGHT: f32 = 34.0;

/// Space above and below the clip rows, so the viewport outline does not
/// sit on top of the first and last track.
const ROW_INSET: f32 = 3.0;
/// Pixels either side of a view box edge that still grab it.
const EDGE_GRAB_PX: f32 = 4.0;
/// The mockup never draws the view box narrower than this, and its middle
/// this wide always pans, so a far zoom can still be dragged.
const MIN_BOX_PX: f32 = 6.0;

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

    /// Where the view box's edges sit in an overview `width` wide.
    fn view_box_x(&self, overview: TimelineGeometry, width: f32) -> (f32, f32) {
        let start = overview.beat_to_x(self.scroll_offset_beats);
        let end = overview.beat_to_x(self.scroll_offset_beats + self.visible_beats(width));
        let widen = ((MIN_BOX_PX - (end - start)) / 2.0).max(0.0);
        (start - widen, end + widen)
    }

    /// The view box edge a press at `x` grabs: whether it is the start, and
    /// the beat the other edge keeps.
    fn edge_at(&self, overview: TimelineGeometry, width: f32, x: f32) -> Option<(bool, f64)> {
        let (start_x, end_x) = self.view_box_x(overview, width);
        let inner_grab = ((end_x - start_x - MIN_BOX_PX) / 2.0).clamp(0.0, EDGE_GRAB_PX);
        if x > start_x + inner_grab && x < end_x - inner_grab {
            return None;
        }
        nearest_end(start_x, end_x, x, EDGE_GRAB_PX).map(|edge| {
            let start = edge == LoopMarker::Start;
            let fixed = if start {
                self.scroll_offset_beats + self.visible_beats(width)
            } else {
                self.scroll_offset_beats
            };
            (start, fixed)
        })
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
    /// Dragging one edge of the viewport rectangle: zooming with the other
    /// edge held at `fixed_beat`.
    ResizeViewport {
        dragging_start: bool,
        fixed_beat: f64,
    },
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

        let background = theme::track_bg();
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), background);

        if self.total_beats <= 0.0 || self.tracks.is_empty() {
            return vec![frame.into_geometry()];
        }

        let overview = TimelineGeometry::fitted(self.total_beats, w, 0.0);
        let loop_span = self
            .loop_enabled
            .then(|| LoopSpan::across(overview, w, (self.loop_start_beats, self.loop_end_beats)))
            .flatten();
        if let Some(span) = loop_span {
            frame.fill_rectangle(
                iced::Point::new(span.start, 0.0),
                iced::Size::new(span.end - span.start, h),
                theme::blend(background, theme::accent(), theme::lane_style().loop_tint),
            );
        }
        let num_tracks = self.tracks.len();
        let rows_h = h - 2.0 * ROW_INSET;
        let track_h = (rows_h / num_tracks as f32).max(1.0);
        let clip_h = if track_h >= 3.0 {
            track_h - 1.0
        } else {
            track_h
        };

        for (i, track) in self.tracks.iter().enumerate() {
            let y = ROW_INSET + i as f32 * track_h;
            let clip_color = theme::blend(track.color, background, 0.15);
            for &(start, dur) in &track.clips {
                let cx = overview.beat_to_x(start);
                let cw = overview.width_for_beats(dur).max(1.0);
                if cx + cw < 0.0 || cx > w {
                    continue;
                }
                frame.fill_rectangle(
                    iced::Point::new(cx, y),
                    iced::Size::new(cw, clip_h),
                    clip_color,
                );
            }
        }

        // Viewport rectangle
        let (vx_start, vx_end) = self.view_box_x(overview, w);
        let vx = vx_start.max(0.0);
        let vw = vx_end.min(w) - vx;
        if vw > 0.0 {
            frame.fill_rectangle(
                iced::Point::new(vx, 0.0),
                iced::Size::new(vw, h),
                theme::with_alpha(theme::text(), 0.008),
            );
            let (left, right) = (vx.round() + 0.5, (vx + vw).round() - 0.5);
            let outline = canvas::Path::rounded_rectangle(
                iced::Point::new(left, 1.5),
                iced::Size::new((right - left).max(1.0), h - 3.0),
                3.0.into(),
            );
            frame.stroke(
                &outline,
                canvas::Stroke::default()
                    .with_color(theme::blend(background, theme::text(), 0.55))
                    .with_width(1.0),
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
                    if let Some((dragging_start, fixed_beat)) =
                        self.edge_at(overview, bounds.width, pos.x)
                    {
                        state.drag = Some(MinimapDragAction::ResizeViewport {
                            dragging_start,
                            fixed_beat,
                        });
                        return (canvas::event::Status::Captured, None);
                    }

                    // Check if click is inside the viewport rectangle
                    let (vx_start, vx_end) = self.view_box_x(overview, bounds.width);

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
                            MinimapDragAction::ResizeViewport {
                                dragging_start,
                                fixed_beat,
                            } => {
                                let pointer = overview.x_to_beat(local_x);
                                let min_span = overview.beats_for_width(2.0 * EDGE_GRAB_PX);
                                let (start, end) = if *dragging_start {
                                    loop_with_start(pointer, *fixed_beat, min_span)
                                } else {
                                    loop_with_end(pointer, *fixed_beat, min_span, self.total_beats)
                                };
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::View(ViewMsg::SetViewport(
                                        TimelineViewport::showing(start, end, bounds.width),
                                    ))),
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
                MinimapDragAction::ResizeViewport { .. } => {
                    mouse::Interaction::ResizingHorizontally
                }
                MinimapDragAction::Seeking => mouse::Interaction::Pointer,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            let overview = TimelineGeometry::fitted(self.total_beats, bounds.width, 0.0);
            if self.edge_at(overview, bounds.width, pos.x).is_some() {
                return mouse::Interaction::ResizingHorizontally;
            }
            let (vx_start, vx_end) = self.view_box_x(overview, bounds.width);

            if pos.x >= vx_start && pos.x <= vx_end {
                return mouse::Interaction::Grab;
            }

            return mouse::Interaction::Pointer;
        }

        mouse::Interaction::default()
    }
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;
