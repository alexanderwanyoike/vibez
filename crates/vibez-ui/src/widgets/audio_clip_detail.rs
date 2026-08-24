use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};

use crate::domains::arrangement::ArrangementMsg;
use crate::message::Message;
use crate::state::GridConfig;
use crate::theme;
use crate::widgets::clip_loop_markers::{self, LoopDrag, LoopMarker, MARKER_RAIL_HEIGHT};
use crate::widgets::local_drag::LocalDrag;

/// Canvas widget for showing a detailed waveform of an audio clip in the detail panel.
pub struct AudioClipDetailWidget {
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub audio: Arc<DecodedAudio>,
    pub duration_samples: u64,
    pub source_offset: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub grid: GridConfig,
    pub track_color: Color,
    /// Normalized playhead position within the clip (0.0..1.0), negative means not in clip.
    pub playhead_normalized: f64,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
}

#[derive(Debug, Default)]
pub struct AudioClipDetailState {
    drag: Option<LoopDrag>,
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

    fn source_to_x(&self, source_frame: u64, bounds: &Rectangle) -> f32 {
        let visible_frames = self.duration_samples.max(1);
        let local = source_frame
            .saturating_sub(self.source_offset)
            .min(visible_frames);
        (local as f64 / visible_frames as f64 * f64::from(bounds.width)) as f32
    }

    fn x_to_local_frame(&self, x: f32, bounds: &Rectangle) -> u64 {
        let fraction = f64::from(x / bounds.width.max(1.0)).clamp(0.0, 1.0);
        let local = fraction * self.duration_samples as f64;
        let local = if self.grid.snap_enabled {
            let beat = local / self.samples_per_beat();
            self.grid.snap_beat(beat, self.pixels_per_beat(bounds)) * self.samples_per_beat()
        } else {
            local
        };
        (local.round() as u64).min(self.loop_range_frames())
    }

    fn hit_test_loop_marker(&self, position: Point, bounds: &Rectangle) -> Option<LoopMarker> {
        if !self.loop_enabled || self.loop_end <= self.loop_start {
            return None;
        }
        clip_loop_markers::hit_test(
            self.source_to_x(self.loop_start, bounds),
            self.source_to_x(self.loop_end, bounds),
            position,
            MARKER_RAIL_HEIGHT,
        )
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
            let line = canvas::Path::line(Point::new(x, MARKER_RAIL_HEIGHT), Point::new(x, h));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(width),
            );
        }

        // Center line
        let waveform_height = (h - MARKER_RAIL_HEIGHT).max(1.0);
        let center_y = MARKER_RAIL_HEIGHT + waveform_height / 2.0;
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
                    let bx = boundary as f32 / num_frames as f32 * w;
                    let line = canvas::Path::line(
                        iced::Point::new(bx, MARKER_RAIL_HEIGHT),
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
                Some(peak_for_range(loop_start, loop_end))
            } else {
                None
            };

            for px in 0..pixels {
                let clip_frame_start = px * num_frames / pixels.max(1);
                let clip_frame_end = (px + 1) * num_frames / pixels.max(1);
                let span = clip_frame_end.saturating_sub(clip_frame_start).max(1);

                let (min_val, max_val) = if !looping {
                    // Non-looped: direct contiguous range
                    let src_start = self.source_offset as usize + clip_frame_start;
                    let src_end = self.source_offset as usize + clip_frame_end;
                    peak_for_range(src_start, src_end)
                } else if span >= loop_len {
                    // Pixel covers at least one full loop cycle — use cached full peak
                    full_loop_peak.unwrap()
                } else {
                    // Map start/end into source positions within the loop
                    let raw_start = self.source_offset as usize + clip_frame_start;
                    let raw_end = self.source_offset as usize + clip_frame_end;
                    let src_start = if raw_start >= loop_end {
                        loop_start + (raw_start - loop_start) % loop_len
                    } else {
                        raw_start
                    };
                    let src_end = if raw_end >= loop_end {
                        loop_start + (raw_end - loop_start) % loop_len
                    } else {
                        raw_end
                    };

                    if src_start <= src_end {
                        // Contiguous segment
                        peak_for_range(src_start, src_end.max(src_start + 1))
                    } else {
                        // Wraps around loop boundary: two segments
                        let (mn1, mx1) = peak_for_range(src_start, loop_end);
                        let (mn2, mx2) = peak_for_range(loop_start, src_end.max(loop_start + 1));
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
            iced::Size::new(w, MARKER_RAIL_HEIGHT),
            theme::with_alpha(theme::bg_surface(), 0.96),
        );
        if looping {
            clip_loop_markers::draw_brace(
                &mut frame,
                self.source_to_x(self.loop_start, &bounds),
                self.source_to_x(self.loop_end, &bounds),
                theme::accent(),
            );
        }

        let ruler_border = canvas::Path::line(
            Point::new(0.0, MARKER_RAIL_HEIGHT),
            Point::new(w, MARKER_RAIL_HEIGHT),
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
                    Point::new(x, MARKER_RAIL_HEIGHT - 6.0),
                    Point::new(x, MARKER_RAIL_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{}", (beat / 4.0) as usize + 1),
                    position: Point::new(x + 3.0, 10.0),
                    color: theme::text_dim(),
                    size: iced::Pixels(8.0),
                    ..Default::default()
                });
            } else if is_beat && pixels_per_beat > 40.0 {
                let tick = canvas::Path::line(
                    Point::new(x, MARKER_RAIL_HEIGHT - 3.0),
                    Point::new(x, MARKER_RAIL_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(0.5),
                );
            }
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
            || cursor
                .position_in(bounds)
                .and_then(|position| self.hit_test_loop_marker(position, &bounds))
                .is_some()
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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(marker) = self.hit_test_loop_marker(position, &bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                state.drag = Some(LoopDrag::begin(
                    marker,
                    self.loop_start.saturating_sub(self.source_offset) as f64,
                    self.loop_end.saturating_sub(self.source_offset) as f64,
                ));
                (canvas::event::Status::Captured, None)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(drag) = state.drag else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(position) = LocalDrag::unclamped().position(cursor, bounds) else {
                    return (canvas::event::Status::Captured, None);
                };
                let pointer = self.x_to_local_frame(position.x, &bounds);
                let max = self.loop_range_frames();
                let min_length = self.minimum_loop_frames(&bounds).min(max);
                let (loop_start, loop_end) =
                    drag.resolve(pointer as f64, min_length as f64, max as f64);
                let loop_start = self.source_offset.saturating_add(loop_start.round() as u64);
                let loop_end = self.source_offset.saturating_add(loop_end.round() as u64);
                if (loop_start, loop_end) == (self.loop_start, self.loop_end) {
                    return (canvas::event::Status::Captured, None);
                }
                (
                    canvas::event::Status::Captured,
                    Some(
                        Message::Arrangement(ArrangementMsg::SetClipLoopRegion {
                            track_id: self.track_id,
                            clip_id: self.clip_id,
                            loop_start,
                            loop_end,
                        })
                        .in_undo_gesture(drag.undo_gesture()),
                    ),
                )
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
mod tests {
    use super::*;
    use iced::widget::canvas::Program;

    fn widget() -> AudioClipDetailWidget {
        AudioClipDetailWidget {
            track_id: TrackId::new(),
            clip_id: ClipId::new(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; 1_000]],
                sample_rate: 1_000,
            }),
            duration_samples: 800,
            source_offset: 100,
            sample_rate: 1_000,
            bpm: 120.0,
            grid: GridConfig::new(crate::state::SnapGrid::QUARTER, true, false, 0),
            track_color: Color::WHITE,
            playhead_normalized: -1.0,
            loop_enabled: true,
            loop_start: 100,
            loop_end: 500,
        }
    }

    fn arrangement_message(message: Option<Message>) -> Option<ArrangementMsg> {
        match message {
            Some(Message::UndoGesture { edit, .. }) => arrangement_message(Some(*edit)),
            Some(Message::Arrangement(message)) => Some(message),
            _ => None,
        }
    }

    #[test]
    fn audio_loop_end_marker_drag_edits_source_frames() {
        let widget = widget();
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
        let end = Point::new(widget.source_to_x(widget.loop_end, &bounds), 5.0);
        let target = Point::new(600.0, 5.0);
        let mut state = AudioClipDetailState::default();

        let pressed = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                mouse::Cursor::Available(end),
            )
            .0;
        assert_eq!(pressed, canvas::event::Status::Captured);

        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
                bounds,
                mouse::Cursor::Available(target),
            )
            .1;
        assert!(matches!(
            arrangement_message(message),
            Some(ArrangementMsg::SetClipLoopRegion {
                track_id,
                clip_id,
                loop_start: 100,
                loop_end: 600,
            }) if track_id == widget.track_id && clip_id == widget.clip_id
        ));
    }

    #[test]
    fn audio_ruler_maps_measures_from_project_tempo() {
        let mut widget = widget();
        widget.duration_samples = 4_000;
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));

        assert_eq!(widget.total_beats(), 8.0);
        assert_eq!(widget.beat_to_x(4.0, &bounds), 400.0);
        assert_eq!(widget.beat_to_x(8.0, &bounds), 800.0);
    }

    #[test]
    fn audio_loop_drag_skips_an_unchanged_snapped_region() {
        let mut widget = widget();
        widget.loop_end = 600;
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
        let end = Point::new(widget.source_to_x(widget.loop_end, &bounds), 5.0);
        let mut state = AudioClipDetailState::default();

        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(end),
        );
        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved { position: end }),
                bounds,
                mouse::Cursor::Available(end),
            )
            .1;

        assert!(message.is_none());
    }

    #[test]
    fn audio_loop_overshoot_keeps_one_grid_cell_between_the_handles() {
        let mut widget = widget();
        widget.loop_end = 900;
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
        let start = Point::new(widget.source_to_x(widget.loop_start, &bounds), 5.0);
        let past_end = Point::new(1_000.0, 5.0);
        let mut state = AudioClipDetailState::default();

        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(start),
        );
        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved { position: past_end }),
                bounds,
                mouse::Cursor::Available(past_end),
            )
            .1;

        assert!(matches!(
            arrangement_message(message),
            Some(ArrangementMsg::SetClipLoopRegion {
                loop_start: 400,
                loop_end: 900,
                ..
            })
        ));
    }

    #[test]
    fn dragging_repairs_a_legacy_region_past_the_visible_clip() {
        let mut widget = widget();
        widget.duration_samples = 400;
        widget.loop_end = 900;
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(800.0, 200.0));
        let end = Point::new(bounds.width - 1.0, 5.0);
        let target = Point::new(600.0, 5.0);
        let mut state = AudioClipDetailState::default();

        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(end),
        );
        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved { position: target }),
                bounds,
                mouse::Cursor::Available(target),
            )
            .1;

        assert!(matches!(
            arrangement_message(message),
            Some(ArrangementMsg::SetClipLoopRegion { loop_end: 500, .. })
        ));
    }
}
