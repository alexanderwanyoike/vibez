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
use crate::widgets::clip_loop_markers::{self, LoopDrag, LoopMarker};
use crate::widgets::double_click::DoubleClick;
use crate::widgets::local_drag::LocalDrag;

/// Canvas widget for showing a detailed waveform of an audio clip in the detail panel.
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
    /// Normalized playhead position within the clip (0.0..1.0), negative means not in clip.
    pub playhead_normalized: f64,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    pub playback_direction: ClipPlaybackDirection,
    pub transient_markers: TransientMarkers,
    pub selected_transient_marker: Option<u64>,
    pub warp_markers: WarpMarkers,
    pub selected_warp_marker: Option<u64>,
}

#[derive(Debug, Default)]
pub struct AudioClipDetailState {
    drag: Option<AudioMarkerDrag>,
    double_click: DoubleClick,
}

const AUDIO_RULER_HEIGHT: f32 = 30.0;
const LOOP_HANDLE_ROW_HEIGHT: f32 = 10.0;

#[derive(Debug, Clone, Copy)]
enum AudioMarkerDrag {
    Start(UndoGestureId),
    Loop(LoopDrag),
    Transient {
        current_source_frame: u64,
        undo_gesture: UndoGestureId,
    },
    Warp {
        source_frame: u64,
        current_timeline_frame: u64,
        undo_gesture: UndoGestureId,
    },
}

const TRANSIENT_HIT_RADIUS: f32 = 6.0;
const WARP_MARKER_TOP: f32 = 20.0;
const WARP_HIT_RADIUS: f32 = 7.0;
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);

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

    fn pixels_per_beat(&self, bounds: &Rectangle) -> f32 {
        let beats = self.total_beats();
        (f64::from(bounds.width) / beats.max(f64::EPSILON)) as f32
    }

    fn total_beats(&self) -> f64 {
        self.duration_samples as f64 / self.samples_per_beat()
    }

    fn beat_to_x(&self, beat: f64, bounds: &Rectangle) -> f32 {
        (beat / self.total_beats().max(f64::EPSILON) * f64::from(bounds.width)) as f32
    }

    fn minimum_loop_frames(&self, bounds: &Rectangle) -> u64 {
        let beats = if self.grid.snap_enabled {
            self.grid
                .effective_grid(self.pixels_per_beat(bounds))
                .beat_size()
        } else {
            0.01
        };
        (beats * self.samples_per_beat()).round().max(1.0) as u64
    }

    fn timeline_to_x(&self, timeline_frame: u64, bounds: &Rectangle) -> f32 {
        let visible_frames = self.duration_samples.max(1);
        let local = timeline_frame
            .saturating_sub(self.source_offset)
            .min(visible_frames);
        let fraction = local as f64 / visible_frames as f64;
        let fraction = match self.playback_direction {
            ClipPlaybackDirection::Forward => fraction,
            ClipPlaybackDirection::Reverse => 1.0 - fraction,
        };
        (fraction * f64::from(bounds.width)) as f32
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
        let fraction = f64::from(x / bounds.width.max(1.0)).clamp(0.0, 1.0);
        let fraction = match self.playback_direction {
            ClipPlaybackDirection::Forward => fraction,
            ClipPlaybackDirection::Reverse => 1.0 - fraction,
        };
        let local = fraction * self.duration_samples as f64;
        let local = if self.grid.snap_enabled {
            let beat = local / self.samples_per_beat();
            self.grid.snap_beat(beat, self.pixels_per_beat(bounds)) * self.samples_per_beat()
        } else {
            local
        };
        (local.round() as u64).min(self.loop_range_frames())
    }

    fn x_to_unsnapped_local_frame(&self, x: f32, bounds: &Rectangle) -> u64 {
        let fraction = f64::from(x / bounds.width.max(1.0)).clamp(0.0, 1.0);
        let fraction = match self.playback_direction {
            ClipPlaybackDirection::Forward => fraction,
            ClipPlaybackDirection::Reverse => 1.0 - fraction,
        };
        ((fraction * self.duration_samples as f64).round() as u64).min(self.loop_range_frames())
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

    fn start_marker_from_x(&self, x: f32, bounds: &Rectangle) -> u64 {
        let candidate = self
            .source_offset
            .saturating_add(self.x_to_local_frame(x, bounds));
        let source_end = self.source_offset.saturating_add(self.loop_range_frames());
        FrameClipTimeline::new(
            self.start_marker,
            self.loop_start,
            self.loop_end,
            self.duration_samples,
            self.loop_enabled,
        )
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

        let total_beats = self.total_beats().max(f64::EPSILON);
        let pixels_per_beat = self.pixels_per_beat(&bounds);
        let grid_step = self.grid.effective_grid(pixels_per_beat).beat_size();
        let grid_steps = (total_beats / grid_step).ceil() as usize;

        // Musical grid behind the waveform. Audio and MIDI editors now
        // describe time with the same bars-and-beats language.
        for step in 0..=grid_steps {
            let beat = step as f64 * grid_step;
            let x = self.beat_to_x(beat, &bounds).floor() + 0.5;
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

        // Draw waveform
        let num_frames = self.duration_samples as usize;
        let looping = self.loop_enabled && self.loop_end > self.loop_start;
        let loop_start = self.loop_start as usize;
        let loop_end = self.loop_end as usize;
        let loop_len = if looping { loop_end - loop_start } else { 0 };
        let mapped_loop_start = self.mapped_source_frame(self.loop_start).floor() as usize;
        let mapped_loop_end = self.mapped_source_frame(self.loop_end).ceil() as usize;
        let timeline = FrameClipTimeline::new(
            self.start_marker,
            self.loop_start,
            self.loop_end,
            self.duration_samples,
            self.loop_enabled,
        );

        if num_frames > 0 {
            let pixels = w as usize;
            let half_h = (waveform_height / 2.0 - 2.0).max(1.0);
            let channels = self.audio.num_channels();
            let waveform_color = theme::with_alpha(self.track_color, 0.7);
            let loop_line_color = theme::with_alpha(self.track_color, 0.35);

            // Draw loop boundary markers if looping
            if looping {
                // The clip's source offset can sit past the loop points
                // (e.g. a trimmed clip start); saturate so markers clamp
                // to the left edge instead of underflowing.
                let source_offset = self.source_offset as usize;
                // The loop region repeats — show a subtle vertical line at each loop boundary
                let mut boundary = loop_end
                    .saturating_sub(source_offset)
                    .saturating_add(loop_len);
                while boundary < num_frames {
                    let forward_x = boundary as f32 / num_frames as f32 * w;
                    let bx = match self.playback_direction {
                        ClipPlaybackDirection::Forward => forward_x,
                        ClipPlaybackDirection::Reverse => w - forward_x,
                    };
                    let line = canvas::Path::line(
                        iced::Point::new(bx, AUDIO_RULER_HEIGHT),
                        iced::Point::new(bx, h),
                    );
                    frame.stroke(
                        &line,
                        canvas::Stroke::default()
                            .with_color(loop_line_color)
                            .with_width(1.0),
                    );
                    boundary += loop_len;
                }
            }

            // Helper: get peak across all channels for a contiguous source range
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

            // When looping, the entire looped waveform is just the source region
            // [loop_start..loop_end) repeated. For any pixel spanning N source frames,
            // if N >= loop_len we know the peak is just the peak of the whole loop region.
            // Otherwise we break into at most 2 contiguous segments.
            let full_loop_peak = if looping {
                Some(peak_for_range(mapped_loop_start, mapped_loop_end))
            } else {
                None
            };

            for px in 0..pixels {
                let visible_start = px * num_frames / pixels.max(1);
                let visible_end = (px + 1) * num_frames / pixels.max(1);
                let (clip_frame_start, clip_frame_end) = match self.playback_direction {
                    ClipPlaybackDirection::Forward => (visible_start, visible_end),
                    ClipPlaybackDirection::Reverse => {
                        (num_frames - visible_end, num_frames - visible_start)
                    }
                };
                let span = clip_frame_end.saturating_sub(clip_frame_start).max(1);

                let (min_val, max_val) = if !looping {
                    // Non-looped: direct contiguous range
                    let src_start = self
                        .mapped_source_frame(timeline.source_at(clip_frame_start as u64))
                        .floor() as usize;
                    let src_end = self
                        .mapped_source_frame(timeline.source_at(clip_frame_end as u64))
                        .ceil() as usize;
                    peak_for_range(src_start, src_end)
                } else if span >= loop_len {
                    // Pixel covers at least one full loop cycle — use cached full peak
                    full_loop_peak.unwrap()
                } else {
                    // Map start/end into source positions within the loop
                    let src_start = self
                        .mapped_source_frame(timeline.source_at(clip_frame_start as u64))
                        .floor() as usize;
                    let src_end = self
                        .mapped_source_frame(timeline.source_at(clip_frame_end as u64))
                        .ceil() as usize;

                    if src_start <= src_end {
                        // Contiguous segment
                        peak_for_range(src_start, src_end.max(src_start + 1))
                    } else {
                        // Wraps around loop boundary: two segments
                        let (mn1, mx1) = peak_for_range(src_start, mapped_loop_end);
                        let (mn2, mx2) =
                            peak_for_range(mapped_loop_start, src_end.max(mapped_loop_start + 1));
                        (mn1.min(mn2), mx1.max(mx2))
                    }
                };

                let y_top = center_y - (max_val * half_h);
                let y_bottom = center_y - (min_val * half_h);
                let height = (y_bottom - y_top).max(1.0);

                frame.fill_rectangle(
                    iced::Point::new(px as f32, y_top),
                    iced::Size::new(1.0, height),
                    waveform_color,
                );
            }
        }

        // Ruler rail overlays the waveform. The brace is painted before
        // labels so measure numbers remain readable where they overlap.
        frame.fill_rectangle(
            Point::ORIGIN,
            iced::Size::new(w, AUDIO_RULER_HEIGHT),
            theme::with_alpha(theme::bg_surface(), 0.96),
        );
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
        for step in 0..=grid_steps {
            let beat = step as f64 * grid_step;
            let x = self.beat_to_x(beat, &bounds).floor() + 0.5;
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

        // Playhead
        if self.playhead_normalized >= 0.0 && self.playhead_normalized <= 1.0 {
            let px = (self.playhead_normalized as f32) * w;
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
        if state.drag.is_some()
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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let source_frame = self.transient_source_from_x(position.x, &bounds);
                let marker = self.hit_test_transient_marker(position, &bounds);
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
                            marker,
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
                    state.double_click.clear();
                    state.drag = Some(AudioMarkerDrag::Transient {
                        current_source_frame: source_frame,
                        undo_gesture: UndoGestureId::new(),
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
                    )))
                } else {
                    return (canvas::event::Status::Ignored, None);
                };
                (canvas::event::Status::Captured, None)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(drag) = state.drag else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(position) = LocalDrag::unclamped().position(cursor, bounds) else {
                    return (canvas::event::Status::Captured, None);
                };
                let message = match drag {
                    AudioMarkerDrag::Start(undo_gesture) => {
                        let start_marker = self.start_marker_from_x(position.x, &bounds);
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
                        let pointer = self.x_to_local_frame(position.x, &bounds);
                        let max = self.loop_range_frames();
                        let min_length = self.minimum_loop_frames(&bounds).min(max);
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
