use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::clip_timeline::FrameClipTimeline;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::ClipPlaybackDirection;
use vibez_core::transient::{TransientMarkerKind, TransientMarkers};
use vibez_core::warp_marker::WarpMarkers;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{ContextMenuTarget, GridConfig, UndoGestureId};
use crate::theme;
use crate::timeline_geometry::{TimelineGeometry, TimelineViewport};
use crate::widgets::clip_editor_view::{wheel_intent, ClipEditorView, RulerZoom, WheelIntent};
use crate::widgets::clip_loop_markers::{self, LoopDrag, LoopMarker};
use crate::widgets::double_click::DoubleClick;
use crate::widgets::local_drag::LocalDrag;

/// Canvas widget for showing a detailed waveform of an audio clip in the detail panel.
///
/// It shows the clip's content, the source from the clip's offset onward,
/// not the clip as it plays in the Arrangement, so a loop can be placed
/// anywhere the source reaches.
pub struct AudioClipDetailWidget {
    pub location: vibez_project::TimelineLocation,
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub audio: Arc<DecodedAudio>,
    pub duration_samples: u64,
    pub source_offset: u64,
    pub start_marker: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub grid: GridConfig,
    pub track_color: Color,
    /// How far into the clip the playhead is, while it is inside the clip.
    pub playhead_frame: Option<u64>,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    /// See `FrameClipTimeline::loop_content_end`.
    pub loop_content_end: u64,
    pub playback_direction: ClipPlaybackDirection,
    pub transient_markers: TransientMarkers,
    pub selected_transient_marker: Option<u64>,
    pub warp_markers: WarpMarkers,
    pub selected_warp_marker: Option<u64>,
    /// The producer's zoom and scroll for this clip; `None` frames its loop.
    pub view: Option<TimelineViewport>,
}

#[derive(Debug, Default)]
pub struct AudioClipDetailState {
    drag: Option<AudioMarkerDrag>,
    double_click: DoubleClick,
    shift_held: bool,
    alt_held: bool,
}

const AUDIO_RULER_HEIGHT: f32 = 30.0;
const LOOP_HANDLE_ROW_HEIGHT: f32 = 10.0;

#[derive(Debug, Clone, Copy)]
enum AudioMarkerDrag {
    Start(UndoGestureId),
    Loop(LoopDrag),
    Ruler(RulerZoom),
    Transient {
        current_source_frame: u64,
        undo_gesture: UndoGestureId,
    },
    TransientWarp {
        source_frame: u64,
        current_timeline_frame: u64,
        undo_gesture: UndoGestureId,
    },
    Warp {
        source_frame: u64,
        current_timeline_frame: u64,
        undo_gesture: UndoGestureId,
    },
}

const TRANSIENT_HIT_RADIUS: f32 = 6.0;
const TRANSIENT_HANDLE_BOTTOM: f32 = AUDIO_RULER_HEIGHT + 8.0;
const WARP_MARKER_TOP: f32 = 20.0;
const WARP_HIT_RADIUS: f32 = 7.0;
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);

/// The audio editor's view, measured in beats from the clip's source
/// offset.
pub(crate) fn audio_clip_editor_view(
    stored: Option<TimelineViewport>,
    samples_per_beat: f64,
    source_offset: u64,
    loop_span: Option<(u64, u64)>,
    duration: u64,
    content_frames: u64,
) -> ClipEditorView {
    let beats = |frames: u64| frames as f64 / samples_per_beat;
    ClipEditorView::framing(
        stored,
        loop_span.map(|(start, end)| {
            (
                beats(start.saturating_sub(source_offset)),
                beats(end.saturating_sub(source_offset)),
            )
        }),
        beats(duration.max(1)),
        beats(content_frames),
    )
}

impl AudioClipDetailWidget {
    fn loop_range_frames(&self) -> u64 {
        self.duration_samples
            .min((self.audio.num_frames() as u64).saturating_sub(self.source_offset))
            .max(1)
    }

    fn samples_per_beat(&self) -> f64 {
        if self.bpm.is_finite() && self.bpm > 0.0 {
            f64::from(self.sample_rate) * 60.0 / self.bpm
        } else {
            f64::from(self.sample_rate).max(1.0)
        }
    }

    /// The source from the clip's offset to the furthest a loop may reach,
    /// or to the clip's end when an unlooped clip runs on into silence.
    fn content_frames(&self) -> u64 {
        let source = self.loop_content_end.saturating_sub(self.source_offset);
        if self.loop_enabled {
            source
        } else {
            source.max(self.duration_samples)
        }
    }

    fn clip_view(&self) -> ClipEditorView {
        audio_clip_editor_view(
            self.view,
            self.samples_per_beat(),
            self.source_offset,
            self.loop_enabled
                .then_some((self.loop_start, self.loop_end)),
            self.duration_samples,
            self.content_frames(),
        )
    }

    fn geometry(&self, bounds: &Rectangle) -> TimelineGeometry {
        self.clip_view().geometry(bounds.width.max(1.0), 0.0)
    }

    fn pixels_per_beat(&self, bounds: &Rectangle) -> f32 {
        self.geometry(bounds).pixels_per_beat()
    }

    /// `free` is a drag with Alt held, which ignores the grid as the
    /// Arrange's drags do.
    fn minimum_loop_frames(&self, bounds: &Rectangle, free: bool) -> u64 {
        let beats = if self.grid.snap_enabled && !free {
            self.grid
                .effective_grid(self.pixels_per_beat(bounds))
                .beat_size()
        } else {
            0.01
        };
        (beats * self.samples_per_beat()).round().max(1.0) as u64
    }

    fn timeline_to_x(&self, timeline_frame: u64, bounds: &Rectangle) -> f32 {
        let local = timeline_frame as f64 - self.source_offset as f64;
        self.geometry(bounds)
            .beat_to_x(local / self.samples_per_beat())
    }

    #[cfg(test)]
    fn x_to_timeline_frame(&self, x: f32, bounds: &Rectangle) -> f64 {
        self.source_offset as f64 + self.geometry(bounds).x_to_beat(x) * self.samples_per_beat()
    }

    fn timeline(&self) -> FrameClipTimeline {
        FrameClipTimeline::new(
            self.start_marker,
            self.loop_start,
            self.loop_end,
            self.duration_samples,
            self.loop_enabled,
        )
    }

    /// The furthest content frame the clip plays: from its start marker for
    /// its length, or to the loop end once it wraps.
    fn played_end(&self) -> u64 {
        let end = self.start_marker.saturating_add(self.duration_samples);
        if self.timeline().is_looping() {
            end.min(self.loop_end)
        } else {
            end
        }
    }

    /// The content frame the playhead is playing, which wraps with the loop.
    fn playhead_timeline_frame(&self) -> Option<u64> {
        let clip_frame = self
            .playback_direction
            .map_clip_frame(self.playhead_frame?, self.duration_samples);
        Some(self.timeline().source_at(clip_frame))
    }

    fn source_end(&self) -> u64 {
        self.warp_markers.source_end(
            self.source_offset
                .saturating_add(self.duration_samples)
                .min(self.audio.num_frames() as u64),
        )
    }

    fn warp_timeline_end(&self) -> u64 {
        self.warp_markers.timeline_end(self.loop_range_frames())
    }

    fn source_to_x(&self, source_frame: u64, bounds: &Rectangle) -> f32 {
        let local = self
            .warp_markers
            .timeline_at_source(source_frame as f64, self.source_offset, self.source_end())
            .round() as u64;
        self.timeline_to_x(self.source_offset.saturating_add(local), bounds)
    }

    fn mapped_source_frame(&self, timeline_frame: u64) -> f64 {
        if self.warp_markers.is_empty() {
            return timeline_frame as f64;
        }
        self.warp_markers.source_at_timeline(
            timeline_frame.saturating_sub(self.source_offset) as f64,
            self.source_offset,
            self.warp_timeline_end(),
        )
    }

    fn x_to_local_frame(&self, x: f32, bounds: &Rectangle) -> u64 {
        self.x_to_local_frame_up_to(x, bounds, self.loop_range_frames(), false)
    }

    fn x_to_local_frame_up_to(&self, x: f32, bounds: &Rectangle, limit: u64, free: bool) -> u64 {
        let geometry = self.geometry(bounds);
        let beat = geometry.x_to_beat(x).max(0.0);
        let beat = if self.grid.snap_enabled && !free {
            self.grid.snap_beat(beat, geometry.pixels_per_beat())
        } else {
            beat
        };
        ((beat * self.samples_per_beat()).round() as u64).min(limit)
    }

    fn x_to_unsnapped_local_frame(&self, x: f32, bounds: &Rectangle) -> u64 {
        let beat = self.geometry(bounds).x_to_beat(x).max(0.0);
        ((beat * self.samples_per_beat()).round() as u64).min(self.loop_range_frames())
    }

    fn hit_test_loop_marker(&self, position: Point, bounds: &Rectangle) -> Option<LoopMarker> {
        if !self.loop_enabled || self.loop_end <= self.loop_start {
            return None;
        }
        clip_loop_markers::hit_test(
            self.timeline_to_x(self.loop_start, bounds),
            self.timeline_to_x(self.loop_end, bounds),
            position,
            LOOP_HANDLE_ROW_HEIGHT,
        )
    }

    fn hit_test_start_marker(&self, position: Point, bounds: &Rectangle) -> bool {
        clip_loop_markers::hit_test_start(self.timeline_to_x(self.start_marker, bounds), position)
    }

    fn start_marker_from_x(&self, x: f32, bounds: &Rectangle, free: bool) -> u64 {
        let candidate = self
            .source_offset
            .saturating_add(self.x_to_local_frame_up_to(x, bounds, self.content_frames(), free));
        let source_end = self.source_offset.saturating_add(self.loop_range_frames());
        self.timeline()
            .clamp_start(candidate, self.source_offset, source_end)
    }

    fn transient_source_from_x(&self, x: f32, bounds: &Rectangle) -> u64 {
        self.warp_markers
            .source_at_timeline(
                self.x_to_unsnapped_local_frame(x, bounds) as f64,
                self.source_offset,
                self.warp_timeline_end(),
            )
            .round() as u64
    }

    fn timeline_frame_at_source(&self, source_frame: u64) -> u64 {
        self.warp_markers
            .timeline_at_source(source_frame as f64, self.source_offset, self.source_end())
            .round() as u64
    }

    fn is_transient_handle(position: Point) -> bool {
        (AUDIO_RULER_HEIGHT..=TRANSIENT_HANDLE_BOTTOM).contains(&position.y)
    }

    fn hit_test_transient_marker(&self, position: Point, bounds: &Rectangle) -> Option<u64> {
        (position.y >= AUDIO_RULER_HEIGHT).then(|| {
            self.transient_markers
                .as_slice()
                .iter()
                .min_by(|left, right| {
                    let left_distance =
                        (position.x - self.source_to_x(left.source_frame(), bounds)).abs();
                    let right_distance =
                        (position.x - self.source_to_x(right.source_frame(), bounds)).abs();
                    left_distance.total_cmp(&right_distance)
                })
                .filter(|marker| {
                    (position.x - self.source_to_x(marker.source_frame(), bounds)).abs()
                        <= TRANSIENT_HIT_RADIUS
                })
                .map(|marker| marker.source_frame())
        })?
    }

    fn hit_test_warp_marker(&self, position: Point, bounds: &Rectangle) -> Option<u64> {
        ((WARP_MARKER_TOP..=AUDIO_RULER_HEIGHT).contains(&position.y)).then(|| {
            self.warp_markers
                .interior()
                .iter()
                .min_by(|left, right| {
                    let left_x = self.timeline_to_x(
                        self.source_offset.saturating_add(left.timeline_frame()),
                        bounds,
                    );
                    let right_x = self.timeline_to_x(
                        self.source_offset.saturating_add(right.timeline_frame()),
                        bounds,
                    );
                    (position.x - left_x)
                        .abs()
                        .total_cmp(&(position.x - right_x).abs())
                })
                .filter(|marker| {
                    let x = self.timeline_to_x(
                        self.source_offset.saturating_add(marker.timeline_frame()),
                        bounds,
                    );
                    (position.x - x).abs() <= WARP_HIT_RADIUS
                })
                .map(|marker| marker.source_frame())
        })?
    }
}

impl canvas::Program<Message> for AudioClipDetailWidget {
    type State = AudioClipDetailState;

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
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::bg_dark());

        let geometry = self.geometry(&bounds);
        let pixels_per_beat = geometry.pixels_per_beat();
        let samples_per_beat = self.samples_per_beat();
        let grid_step = self.grid.effective_grid(pixels_per_beat).beat_size();
        let first_step = (geometry.x_to_beat(0.0) / grid_step).floor().max(0.0) as usize;
        let last_step = (geometry.x_to_beat(w) / grid_step).ceil().max(0.0) as usize;

        // Musical grid behind the waveform. Audio and MIDI editors now
        // describe time with the same bars-and-beats language.
        for step in first_step..=last_step {
            let beat = step as f64 * grid_step;
            let x = geometry.beat_to_x(beat).floor() + 0.5;
            if x > w {
                break;
            }
            let beat_millis = (beat * 1_000.0).round() as i64;
            let (color, width) = if beat_millis % 4_000 == 0 {
                (theme::grid_bar(), 1.5)
            } else if beat_millis % 1_000 == 0 {
                (theme::grid_beat(), 1.0)
            } else {
                (theme::grid_sub(), 1.0)
            };
            let line = canvas::Path::line(Point::new(x, AUDIO_RULER_HEIGHT), Point::new(x, h));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(width),
            );
        }

        // Center line
        let waveform_height = (h - AUDIO_RULER_HEIGHT).max(1.0);
        let center_y = AUDIO_RULER_HEIGHT + waveform_height / 2.0;
        let center_line = canvas::Path::line(
            iced::Point::new(0.0, center_y),
            iced::Point::new(w, center_y),
        );
        frame.stroke(
            &center_line,
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.3,
                    ..theme::text_dim()
                })
                .with_width(1.0),
        );

        let looping = self.loop_enabled && self.loop_end > self.loop_start;
        let content_frames = self.content_frames() as f64;
        let half_h = (waveform_height / 2.0 - 2.0).max(1.0);
        let channels = self.audio.num_channels();
        let waveform_color = theme::with_alpha(self.track_color, 0.7);
        let peak_for_range = |src_start: usize, src_end: usize| -> (f32, f32) {
            let mut mn = 0.0f32;
            let mut mx = 0.0f32;
            for ch in 0..channels {
                let (ch_min, ch_max) = self.audio.peak_in_range(ch, src_start, src_end);
                mn = mn.min(ch_min);
                mx = mx.max(ch_max);
            }
            (mn, mx)
        };
        for px in 0..w.max(0.0) as usize {
            let local_start = (geometry.x_to_beat(px as f32) * samples_per_beat).max(0.0);
            let local_end =
                (geometry.x_to_beat(px as f32 + 1.0) * samples_per_beat).min(content_frames);
            if local_end <= local_start {
                continue;
            }
            let src_start = self
                .mapped_source_frame(self.source_offset + local_start.floor() as u64)
                .floor() as usize;
            let src_end = self
                .mapped_source_frame(self.source_offset + local_end.ceil() as u64)
                .ceil() as usize;
            let (min_val, max_val) = peak_for_range(src_start, src_end.max(src_start + 1));
            let y_top = center_y - (max_val * half_h);
            let y_bottom = center_y - (min_val * half_h);
            frame.fill_rectangle(
                iced::Point::new(px as f32, y_top),
                iced::Size::new(1.0, (y_bottom - y_top).max(1.0)),
                waveform_color,
            );
        }

        // Ruler rail overlays the waveform. The brace is painted before
        // labels so measure numbers remain readable where they overlap.
        frame.fill_rectangle(
            Point::ORIGIN,
            iced::Size::new(w, AUDIO_RULER_HEIGHT),
            theme::with_alpha(theme::bg_surface(), 0.96),
        );
        let clip_end_x = self.timeline_to_x(self.played_end(), &bounds).max(0.0);
        if clip_end_x < w {
            frame.fill_rectangle(
                Point::new(clip_end_x, 0.0),
                iced::Size::new(w - clip_end_x, h),
                theme::past_clip_end_shade(),
            );
        }
        if looping {
            clip_loop_markers::draw_brace(
                &mut frame,
                self.timeline_to_x(self.loop_start, &bounds),
                self.timeline_to_x(self.loop_end, &bounds),
                theme::accent(),
            );
        }

        let start_x = self.timeline_to_x(self.start_marker, &bounds);
        clip_loop_markers::draw_start(
            &mut frame,
            start_x,
            h,
            theme::text_dim(),
            theme::bg_surface(),
            self.playback_direction == ClipPlaybackDirection::Forward,
        );

        let ruler_border = canvas::Path::line(
            Point::new(0.0, AUDIO_RULER_HEIGHT),
            Point::new(w, AUDIO_RULER_HEIGHT),
        );
        frame.stroke(
            &ruler_border,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );
        for step in first_step..=last_step {
            let beat = step as f64 * grid_step;
            let x = geometry.beat_to_x(beat).floor() + 0.5;
            if x > w {
                break;
            }
            let beat_millis = (beat * 1_000.0).round() as i64;
            let is_bar = beat_millis % 4_000 == 0;
            let is_beat = beat_millis % 1_000 == 0;
            if is_bar {
                let tick = canvas::Path::line(
                    Point::new(x, AUDIO_RULER_HEIGHT - 6.0),
                    Point::new(x, AUDIO_RULER_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{}", (beat / 4.0) as usize + 1),
                    position: Point::new(x + 3.0, 29.0),
                    color: theme::text_dim(),
                    size: iced::Pixels(8.0),
                    ..Default::default()
                });
            } else if is_beat && pixels_per_beat > 40.0 {
                let tick = canvas::Path::line(
                    Point::new(x, AUDIO_RULER_HEIGHT - 3.0),
                    Point::new(x, AUDIO_RULER_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(0.5),
                );
            }
        }

        // Warp Markers own the audible timing map. Their ruler handles are
        // intentionally distinct from the quieter Transient suggestions.
        for marker in self.warp_markers.interior() {
            let x = self
                .timeline_to_x(
                    self.source_offset.saturating_add(marker.timeline_frame()),
                    &bounds,
                )
                .floor()
                + 0.5;
            let selected = self.selected_warp_marker == Some(marker.source_frame());
            let color = if selected {
                theme::meter_yellow()
            } else {
                theme::accent()
            };
            let line = canvas::Path::line(Point::new(x, WARP_MARKER_TOP), Point::new(x, h));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(theme::with_alpha(color, if selected { 0.9 } else { 0.65 }))
                    .with_width(if selected { 2.0 } else { 1.0 }),
            );
            let handle = canvas::Path::new(|path| {
                path.move_to(Point::new(x - 4.0, WARP_MARKER_TOP));
                path.line_to(Point::new(x + 4.0, WARP_MARKER_TOP));
                path.line_to(Point::new(x, AUDIO_RULER_HEIGHT));
                path.close();
            });
            frame.fill(&handle, color);
        }

        // Transient Markers sit over the waveform but under the playhead.
        // Suggested detections are quieter than markers the producer authored
        // or moved by hand.
        for marker in self.transient_markers.as_slice() {
            let x = self.source_to_x(marker.source_frame(), &bounds).floor() + 0.5;
            let selected = self.selected_transient_marker == Some(marker.source_frame());
            let color = if selected {
                theme::meter_yellow()
            } else if marker.kind() == TransientMarkerKind::Authored {
                theme::accent()
            } else {
                theme::with_alpha(theme::accent(), 0.48)
            };
            let line = canvas::Path::line(Point::new(x, AUDIO_RULER_HEIGHT), Point::new(x, h));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(if selected { 2.0 } else { 1.0 }),
            );
            let handle = canvas::Path::new(|path| {
                path.move_to(Point::new(x - 4.0, AUDIO_RULER_HEIGHT));
                path.line_to(Point::new(x + 4.0, AUDIO_RULER_HEIGHT));
                path.line_to(Point::new(x, AUDIO_RULER_HEIGHT + 6.0));
                path.close();
            });
            frame.fill(&handle, color);
        }

        if let Some(px) = self
            .playhead_timeline_frame()
            .map(|frame| self.timeline_to_x(frame, &bounds))
            .filter(|px| (0.0..=w).contains(px))
        {
            let playhead_line =
                canvas::Path::line(iced::Point::new(px, 0.0), iced::Point::new(px, h));
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(2.0),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match state.drag {
            Some(AudioMarkerDrag::Loop(LoopDrag::Move { .. })) => {
                return mouse::Interaction::Grabbing
            }
            Some(AudioMarkerDrag::Ruler(_)) => return mouse::Interaction::ZoomIn,
            _ => {}
        }
        let over_body = cursor.position_in(bounds).is_some_and(|position| {
            self.hit_test_loop_marker(position, &bounds) == Some(LoopMarker::Body)
        });
        if over_body && state.drag.is_none() {
            mouse::Interaction::Grab
        } else if state.drag.is_some()
            || cursor.position_in(bounds).is_some_and(|position| {
                self.hit_test_start_marker(position, &bounds)
                    || self.hit_test_loop_marker(position, &bounds).is_some()
                    || self.hit_test_warp_marker(position, &bounds).is_some()
                    || self.hit_test_transient_marker(position, &bounds).is_some()
            })
        {
            mouse::Interaction::ResizingHorizontally
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
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
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                state.alt_held = modifiers.alt();
                (canvas::event::Status::Ignored, None)
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) =>
            {
                let width = bounds.width.max(1.0);
                let anchor_x = cursor
                    .position_in(bounds)
                    .map_or(width / 2.0, |position| position.x);
                let view = self.clip_view();
                let viewport = match wheel_intent(delta, state.shift_held) {
                    WheelIntent::Zoom(factor) => view.zoomed(factor, anchor_x, width),
                    WheelIntent::ScrollTime(pixels) | WheelIntent::Plain(pixels) => {
                        view.scrolled(pixels, width)
                    }
                };
                (
                    canvas::event::Status::Captured,
                    Some(Message::View(ViewMsg::SetClipEditorView {
                        clip_id: self.clip_id,
                        viewport,
                    })),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let source_frame = self.transient_source_from_x(position.x, &bounds);
                let transient_marker = self.hit_test_transient_marker(position, &bounds);
                let warp_marker = self.hit_test_warp_marker(position, &bounds);
                (
                    canvas::event::Status::Captured,
                    Some(Message::View(ViewMsg::ShowContextMenu {
                        x: bounds.x + position.x,
                        y: bounds.y + position.y,
                        target: ContextMenuTarget::AudioClipDetail {
                            location: self.location,
                            track_id: self.track_id,
                            clip_id: self.clip_id,
                            source_frame,
                            timeline_frame: self.timeline_frame_at_source(source_frame),
                            transient_marker,
                            warp_marker,
                        },
                    })),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                if let Some(source_frame) = self.hit_test_warp_marker(position, &bounds) {
                    let current_timeline_frame = self
                        .warp_markers
                        .interior()
                        .iter()
                        .find(|marker| marker.source_frame() == source_frame)
                        .map(|marker| marker.timeline_frame())
                        .unwrap_or_default();
                    state.double_click.clear();
                    state.drag = Some(AudioMarkerDrag::Warp {
                        source_frame,
                        current_timeline_frame,
                        undo_gesture: UndoGestureId::new(),
                    });
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Arrangement(ArrangementMsg::SelectWarpMarker {
                            track_id: self.track_id,
                            clip_id: self.clip_id,
                            source_frame: Some(source_frame),
                        })),
                    );
                }
                if let Some(source_frame) = self.hit_test_transient_marker(position, &bounds) {
                    let double = state.double_click.press(
                        Instant::now(),
                        position,
                        DOUBLE_CLICK_WINDOW,
                        Some(8.0),
                    );
                    if double {
                        state.double_click.clear();
                        state.drag = None;
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Arrangement(ArrangementMsg::AddWarpMarker {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                source_frame,
                                timeline_frame: self.timeline_frame_at_source(source_frame),
                            })),
                        );
                    }
                    let undo_gesture = UndoGestureId::new();
                    state.drag = Some(if Self::is_transient_handle(position) {
                        AudioMarkerDrag::TransientWarp {
                            source_frame,
                            current_timeline_frame: self.timeline_frame_at_source(source_frame),
                            undo_gesture,
                        }
                    } else {
                        AudioMarkerDrag::Transient {
                            current_source_frame: source_frame,
                            undo_gesture,
                        }
                    });
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Arrangement(
                            ArrangementMsg::SelectTransientMarker {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                source_frame: Some(source_frame),
                            },
                        )),
                    );
                }
                if position.y >= AUDIO_RULER_HEIGHT {
                    let double = state.double_click.press(
                        Instant::now(),
                        position,
                        DOUBLE_CLICK_WINDOW,
                        Some(8.0),
                    );
                    if double {
                        state.double_click.clear();
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Arrangement(ArrangementMsg::AddTransientMarker {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                source_frame: self.transient_source_from_x(position.x, &bounds),
                            })),
                        );
                    }
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Arrangement(
                            ArrangementMsg::SelectTransientMarker {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                source_frame: None,
                            },
                        )),
                    );
                }
                state.drag = if self.hit_test_start_marker(position, &bounds) {
                    Some(AudioMarkerDrag::Start(UndoGestureId::new()))
                } else if let Some(marker) = self.hit_test_loop_marker(position, &bounds) {
                    Some(AudioMarkerDrag::Loop(LoopDrag::begin(
                        marker,
                        self.loop_start.saturating_sub(self.source_offset) as f64,
                        self.loop_end.saturating_sub(self.source_offset) as f64,
                        self.x_to_local_frame_up_to(position.x, &bounds, u64::MAX, state.alt_held)
                            as f64,
                    )))
                } else {
                    Some(AudioMarkerDrag::Ruler(RulerZoom::begin(
                        self.clip_view(),
                        position,
                        bounds.width.max(1.0),
                    )))
                };
                let pin = self.clip_view().pin(bounds.width.max(1.0)).map(|viewport| {
                    Message::View(ViewMsg::SetClipEditorView {
                        clip_id: self.clip_id,
                        viewport,
                    })
                });
                (canvas::event::Status::Captured, pin)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(drag) = state.drag else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(position) = LocalDrag::unclamped().position(cursor, bounds) else {
                    return (canvas::event::Status::Captured, None);
                };
                let message = match drag {
                    AudioMarkerDrag::Ruler(zoom) => {
                        let viewport = zoom.drag_to(position, bounds.width.max(1.0));
                        Some(Message::View(ViewMsg::SetClipEditorView {
                            clip_id: self.clip_id,
                            viewport,
                        }))
                    }
                    AudioMarkerDrag::Start(undo_gesture) => {
                        let start_marker =
                            self.start_marker_from_x(position.x, &bounds, state.alt_held);
                        (start_marker != self.start_marker).then(|| {
                            Message::Arrangement(ArrangementMsg::SetClipStartMarker {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                start_marker,
                            })
                            .in_undo_gesture(undo_gesture)
                        })
                    }
                    AudioMarkerDrag::Loop(drag) => {
                        let max = self.loop_content_end.saturating_sub(self.source_offset);
                        let pointer =
                            self.x_to_local_frame_up_to(position.x, &bounds, max, state.alt_held);
                        let min_length = self.minimum_loop_frames(&bounds, state.alt_held).min(max);
                        let (loop_start, loop_end) =
                            drag.resolve(pointer as f64, min_length as f64, max as f64);
                        let loop_start =
                            self.source_offset.saturating_add(loop_start.round() as u64);
                        let loop_end = self.source_offset.saturating_add(loop_end.round() as u64);
                        ((loop_start, loop_end) != (self.loop_start, self.loop_end)).then(|| {
                            Message::Arrangement(ArrangementMsg::SetClipLoopRegion {
                                track_id: self.track_id,
                                clip_id: self.clip_id,
                                loop_start,
                                loop_end,
                            })
                            .in_undo_gesture(drag.undo_gesture())
                        })
                    }
                    AudioMarkerDrag::Transient {
                        current_source_frame,
                        undo_gesture,
                    } => {
                        state.double_click.clear();
                        let to = self.transient_source_from_x(position.x, &bounds);
                        if to == current_source_frame {
                            None
                        } else {
                            state.drag = Some(AudioMarkerDrag::Transient {
                                current_source_frame: to,
                                undo_gesture,
                            });
                            Some(
                                Message::Arrangement(ArrangementMsg::MoveTransientMarker {
                                    track_id: self.track_id,
                                    clip_id: self.clip_id,
                                    from: current_source_frame,
                                    to,
                                })
                                .in_undo_gesture(undo_gesture),
                            )
                        }
                    }
                    AudioMarkerDrag::TransientWarp {
                        source_frame,
                        current_timeline_frame,
                        undo_gesture,
                    } => {
                        state.double_click.clear();
                        let timeline_frame = self.x_to_local_frame(position.x, &bounds);
                        if timeline_frame == current_timeline_frame {
                            None
                        } else {
                            state.drag = Some(AudioMarkerDrag::Warp {
                                source_frame,
                                current_timeline_frame: timeline_frame,
                                undo_gesture,
                            });
                            Some(
                                Message::Arrangement(ArrangementMsg::AddWarpMarker {
                                    track_id: self.track_id,
                                    clip_id: self.clip_id,
                                    source_frame,
                                    timeline_frame,
                                })
                                .in_undo_gesture(undo_gesture),
                            )
                        }
                    }
                    AudioMarkerDrag::Warp {
                        source_frame,
                        current_timeline_frame,
                        undo_gesture,
                    } => {
                        let timeline_frame = self.x_to_local_frame(position.x, &bounds);
                        if timeline_frame == current_timeline_frame {
                            None
                        } else {
                            state.drag = Some(AudioMarkerDrag::Warp {
                                source_frame,
                                current_timeline_frame: timeline_frame,
                                undo_gesture,
                            });
                            Some(
                                Message::Arrangement(ArrangementMsg::MoveWarpMarker {
                                    track_id: self.track_id,
                                    clip_id: self.clip_id,
                                    source_frame,
                                    timeline_frame,
                                })
                                .in_undo_gesture(undo_gesture),
                            )
                        }
                    }
                };
                (canvas::event::Status::Captured, message)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                (canvas::event::Status::Captured, None)
            }
            _ => (canvas::event::Status::Ignored, None),
        }
    }
}

#[cfg(test)]
#[path = "audio_clip_detail_tests.rs"]
mod tests;
