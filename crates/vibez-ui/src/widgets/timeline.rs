use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::{ArrangementSelection, UiTrack};
use crate::theme;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::TrackKind;

// ── Lightweight data types for rendering ──

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
    pub clip_id: ClipId,
    pub position: u64,
    pub duration: u64,
    pub name: String,
    /// Pre-computed waveform peaks for mini display (per pixel column).
    pub peaks: Vec<(f32, f32)>,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
}

/// Lightweight copy of a note clip for timeline rendering.
pub struct TimelineNoteClip {
    pub clip_id: ClipId,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub name: String,
    pub notes: Vec<(u8, f64, f64)>, // (pitch, start_beat, duration_beats)
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// Compute waveform peaks for a clip, with loop-aware wrapping.
/// Uses `peak_in_range` on contiguous segments for O(pixels) cost regardless of clip length.
pub fn compute_clip_peaks(clip: &crate::state::UiClip) -> Vec<(f32, f32)> {
    let num_peaks = (clip.duration as usize / 100).clamp(1, 1000);
    let looping = clip.loop_enabled && clip.loop_end > clip.loop_start;
    let loop_start = clip.loop_start as usize;
    let loop_end = clip.loop_end as usize;
    let loop_len = if looping { loop_end - loop_start } else { 0 };
    let channels = clip.audio.num_channels();
    if channels == 0 {
        return vec![(0.0, 0.0); num_peaks];
    }

    let peak_for_range = |src_start: usize, src_end: usize| -> (f32, f32) {
        let mut mn = 0.0f32;
        let mut mx = 0.0f32;
        for ch in 0..channels {
            let (ch_min, ch_max) = clip.audio.peak_in_range(ch, src_start, src_end);
            mn = mn.min(ch_min);
            mx = mx.max(ch_max);
        }
        (mn, mx)
    };

    // Cache full loop region peak for spans >= loop_len
    let full_loop_peak = if looping {
        Some(peak_for_range(loop_start, loop_end))
    } else {
        None
    };

    (0..num_peaks)
        .map(|i| {
            let cf_start = i * clip.duration as usize / num_peaks;
            let cf_end = (i + 1) * clip.duration as usize / num_peaks;
            let span = cf_end.saturating_sub(cf_start).max(1);

            if !looping {
                let src_start = clip.source_offset as usize + cf_start;
                let src_end = clip.source_offset as usize + cf_end;
                peak_for_range(src_start, src_end)
            } else if span >= loop_len {
                full_loop_peak.unwrap()
            } else {
                let raw_start = clip.source_offset as usize + cf_start;
                let raw_end = clip.source_offset as usize + cf_end;
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
                    peak_for_range(src_start, src_end.max(src_start + 1))
                } else {
                    // Wraps around loop boundary
                    let (mn1, mx1) = peak_for_range(src_start, loop_end);
                    let (mn2, mx2) = peak_for_range(loop_start, src_end.max(loop_start + 1));
                    (mn1.min(mn2), mx1.max(mx2))
                }
            }
        })
        .collect()
}

// ── RulerWidget ──

/// Canvas widget that draws a beat-based ruler with bar.beat labels.
pub struct RulerWidget {
    pub playhead_beats: f64,
    pub bpm: f64,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
}

impl RulerWidget {
    fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    fn visible_beats(&self, width: f32) -> f64 {
        width as f64 / self.pixels_per_beat() as f64
    }

    fn beat_to_x(&self, beat: f64, _width: f32) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }
}

impl canvas::Program<Message> for RulerWidget {
    type State = ();

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
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::RULER_BG);

        if self.bpm > 0.0 {
            let beats_per_bar = 4.0_f64;
            let visible = self.visible_beats(w);
            let start_beat = self.scroll_offset_beats.floor().max(0.0) as i64;
            let end_beat = (self.scroll_offset_beats + visible).ceil() as i64 + 1;

            // Adaptive label density based on zoom
            let ppb = self.pixels_per_beat();

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
                    // Bar line (thick)
                    let tick =
                        canvas::Path::line(iced::Point::new(x, h * 0.3), iced::Point::new(x, h));
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::BORDER)
                            .with_width(1.5),
                    );

                    // Bar label — always show bar number
                    if ppb < 40.0 {
                        // Low zoom: bar numbers only ("1", "2", "3")
                        let label = format!("{}", bar_index + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 4.0, 3.0),
                            color: theme::RULER_TEXT,
                            size: iced::Pixels(12.0),
                            ..Default::default()
                        });
                    } else {
                        // Medium/high zoom: bar.beat ("1.1", "2.1")
                        let label = format!("{}.1", bar_index + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 4.0, 3.0),
                            color: theme::RULER_TEXT,
                            size: iced::Pixels(12.0),
                            ..Default::default()
                        });
                    }
                } else if ppb >= 40.0 {
                    // Beat ticks only at medium+ zoom
                    let tick =
                        canvas::Path::line(iced::Point::new(x, h * 0.65), iced::Point::new(x, h));
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );

                    // Beat labels only at high zoom (≥80 ppb)
                    if ppb >= 80.0 {
                        let label = format!("{}.{}", bar_index + 1, beat_in_bar + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 2.0, 6.0),
                            color: theme::TEXT_MUTED,
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
                                    .with_color(theme::DIVIDER)
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
                    .with_color(theme::BORDER)
                    .with_width(1.0),
            );
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
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Click to seek: convert pixel x → beat → normalized
                    let ppb = self.pixels_per_beat();
                    let beat = pos.x as f64 / ppb as f64 + self.scroll_offset_beats;
                    if self.total_beats > 0.0 {
                        let normalized = (beat / self.total_beats).clamp(0.0, 1.0);
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Seek(normalized)),
                        );
                    }
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let dy = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => y,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
                    };
                    if dy > 0.0 {
                        return (canvas::event::Status::Captured, Some(Message::ZoomIn));
                    } else if dy < 0.0 {
                        return (canvas::event::Status::Captured, Some(Message::ZoomOut));
                    }
                }
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }
}

// ── TrackClipCanvas ──

/// Pixel threshold for resize handle on right edge of clip.
const RESIZE_EDGE_PX: f32 = 8.0;

/// Drag action in progress on the clip canvas.
#[derive(Debug, Clone)]
pub enum ClipDragAction {
    MoveClip {
        clip_id: ClipId,
        is_note_clip: bool,
        start_beat: f64,
        original_position_beats: f64,
        start_y: f32,
    },
    ResizeClip {
        clip_id: ClipId,
        is_note_clip: bool,
        start_x: f32,
        original_duration_beats: f64,
    },
}

/// Interaction state for clip canvas.
#[derive(Debug, Default)]
pub struct ClipInteractionState {
    pub drag: Option<ClipDragAction>,
}

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
    pub track_id: TrackId,
    pub track_index: usize,
    pub total_tracks: usize,
    pub track_ids: Vec<TrackId>,
    pub track_kinds: Vec<bool>, // is_instrument flags
    pub selected_clip: Option<ClipId>,
    pub clips: Vec<TimelineClip>,
    pub note_clips: Vec<TimelineNoteClip>,
    pub playhead_beats: f64,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub selected: bool,
    pub track_color: Color,
    pub is_instrument: bool,
}

impl TrackClipCanvas {
    #[allow(clippy::too_many_arguments)]
    pub fn from_track(
        track: &UiTrack,
        playhead_beats: f64,
        zoom_level: f32,
        scroll_offset_beats: f64,
        total_beats: f64,
        sample_rate: u32,
        selected: bool,
        track_color: Color,
        bpm: f64,
        track_id: TrackId,
        track_index: usize,
        total_tracks: usize,
        track_ids: Vec<TrackId>,
        track_kinds: Vec<bool>,
        selected_clip: Option<ClipId>,
    ) -> Self {
        let clips = track
            .clips
            .iter()
            .map(|c| TimelineClip {
                clip_id: c.id,
                position: c.position,
                duration: c.duration,
                name: c.name.clone(),
                peaks: compute_clip_peaks(c),
                loop_enabled: c.loop_enabled,
                loop_start: c.loop_start,
                loop_end: c.loop_end,
            })
            .collect();
        let note_clips = track
            .note_clips
            .iter()
            .map(|c| TimelineNoteClip {
                clip_id: c.id,
                position_beats: c.position_beats,
                duration_beats: c.duration_beats,
                name: c.name.clone(),
                notes: c
                    .notes
                    .iter()
                    .map(|n| (n.pitch, n.start_beat, n.duration_beats))
                    .collect(),
                loop_enabled: c.loop_enabled,
                loop_start_beats: c.loop_start_beats,
                loop_end_beats: c.loop_end_beats,
            })
            .collect();
        Self {
            track_id,
            track_index,
            total_tracks,
            track_ids,
            track_kinds,
            selected_clip,
            clips,
            note_clips,
            playhead_beats,
            zoom_level,
            scroll_offset_beats,
            total_beats,
            sample_rate,
            bpm,
            selected,
            track_color,
            is_instrument: matches!(track.kind, TrackKind::Instrument(_)),
        }
    }

    fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    fn x_to_beat(&self, x: f32) -> f64 {
        x as f64 / self.pixels_per_beat() as f64 + self.scroll_offset_beats
    }

    /// Samples per beat.
    fn spb(&self) -> f64 {
        if self.bpm > 0.0 {
            self.sample_rate as f64 * 60.0 / self.bpm
        } else {
            1.0
        }
    }

    /// Hit test: find a clip at the given pixel x position.
    /// Returns (clip_id, is_note_clip, near_right_edge, position_beats, duration_beats).
    fn hit_test(&self, pos_x: f32) -> Option<(ClipId, bool, bool, f64, f64)> {
        let ppb = self.pixels_per_beat();
        let spb = self.spb();

        // Check audio clips
        for clip in &self.clips {
            let clip_start_beat = clip.position as f64 / spb;
            let clip_dur_beats = clip.duration as f64 / spb;
            let clip_x = self.beat_to_x(clip_start_beat);
            let clip_w = (clip_dur_beats * ppb as f64) as f32;

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    clip.clip_id,
                    false,
                    near_right,
                    clip_start_beat,
                    clip_dur_beats,
                ));
            }
        }

        // Check note clips
        for note_clip in &self.note_clips {
            let clip_x = self.beat_to_x(note_clip.position_beats);
            let clip_w = (note_clip.duration_beats * ppb as f64) as f32;

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    note_clip.clip_id,
                    true,
                    near_right,
                    note_clip.position_beats,
                    note_clip.duration_beats,
                ));
            }
        }

        None
    }
}

impl canvas::Program<Message> for TrackClipCanvas {
    type State = ClipInteractionState;

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
        let ppb = self.pixels_per_beat();

        // Background
        let bg_color = if self.selected {
            theme::TRACK_BG_SELECTED
        } else {
            theme::TRACK_BG
        };
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), bg_color);

        // Grid lines — adaptive density matching ruler
        if self.bpm > 0.0 {
            let visible = w as f64 / ppb as f64;
            let start = self.scroll_offset_beats.floor().max(0.0) as i64;
            let end = (self.scroll_offset_beats + visible).ceil() as i64 + 1;

            for beat_i in start..end {
                let x = self.beat_to_x(beat_i as f64);
                if x < -1.0 || x > w + 1.0 {
                    continue;
                }
                let is_bar = beat_i % 4 == 0;

                if is_bar {
                    // Bar lines always visible
                    let vline =
                        canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                    frame.stroke(
                        &vline,
                        canvas::Stroke::default()
                            .with_color(theme::BORDER)
                            .with_width(1.0),
                    );
                } else if ppb >= 40.0 {
                    // Beat lines only at medium+ zoom
                    let vline =
                        canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                    frame.stroke(
                        &vline,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );
                }

                // Sub-beat lines at high zoom (≥80 ppb)
                if ppb >= 80.0 {
                    for sub in 1..4 {
                        let sub_beat = beat_i as f64 + sub as f64 * 0.25;
                        let sub_x = self.beat_to_x(sub_beat);
                        if sub_x > 0.0 && sub_x < w {
                            let sub_line = canvas::Path::line(
                                iced::Point::new(sub_x, 0.0),
                                iced::Point::new(sub_x, h),
                            );
                            frame.stroke(
                                &sub_line,
                                canvas::Stroke::default()
                                    .with_color(Color {
                                        r: 0.13,
                                        g: 0.13,
                                        b: 0.13,
                                        a: 1.0,
                                    })
                                    .with_width(0.3),
                            );
                        }
                    }
                }
            }
        }

        // Draw audio clips
        if self.bpm > 0.0 {
            let spb = self.sample_rate as f64 * 60.0 / self.bpm;
            let clip_color = theme::with_alpha(self.track_color, 0.5);
            let clip_border_color = theme::darken(self.track_color, 0.7);
            let waveform_color = theme::with_alpha(self.track_color, 0.6);

            for clip in &self.clips {
                let clip_start_beat = clip.position as f64 / spb;
                let clip_dur_beats = clip.duration as f64 / spb;

                let clip_x = self.beat_to_x(clip_start_beat);
                let clip_w = (clip_dur_beats * ppb as f64) as f32;

                // Skip clips entirely outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = 4.0;
                let clip_h = h - 8.0;

                // Clip body
                frame.fill_rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                    clip_color,
                );

                // Mini waveform using track color
                if !clip.peaks.is_empty() && clip_w > 4.0 {
                    let center_y = clip_y + clip_h / 2.0;
                    let half_h = clip_h / 2.0 - 2.0;
                    let pixels = clip_w as usize;
                    for px in 0..pixels {
                        let screen_x = clip_x + px as f32;
                        if screen_x < 0.0 || screen_x > w {
                            continue;
                        }
                        let peak_idx = px * clip.peaks.len() / pixels.max(1);
                        if peak_idx >= clip.peaks.len() {
                            break;
                        }
                        let (min_val, max_val) = clip.peaks[peak_idx];
                        let y_top = center_y - (max_val * half_h);
                        let y_bottom = center_y - (min_val * half_h);
                        let height = (y_bottom - y_top).max(1.0);
                        frame.fill_rectangle(
                            iced::Point::new(screen_x, y_top),
                            iced::Size::new(1.0, height),
                            waveform_color,
                        );
                    }
                }

                // Loop markers
                if clip.loop_enabled && clip.loop_end > clip.loop_start {
                    let loop_region_samples = (clip.loop_end - clip.loop_start) as f64;
                    let loop_region_beats = loop_region_samples / spb;

                    // Draw repeated sections with lower opacity
                    let repeat_color = theme::with_alpha(self.track_color, 0.25);
                    let first_loop_offset_beats = (clip.loop_end - clip.loop_start) as f64 / spb;
                    let mut repeat_beat = clip_start_beat + first_loop_offset_beats;
                    while repeat_beat < clip_start_beat + clip_dur_beats {
                        let rx = self.beat_to_x(repeat_beat);
                        let rw = (loop_region_beats * ppb as f64) as f32;
                        if rx < w && rx + rw > 0.0 {
                            frame.fill_rectangle(
                                iced::Point::new(rx, clip_y),
                                iced::Size::new(rw.min(clip_x + clip_w - rx).max(0.0), clip_h),
                                repeat_color,
                            );
                        }
                        repeat_beat += loop_region_beats;
                    }

                    // Small "L" icon
                    if clip_w > 20.0 {
                        frame.fill_text(canvas::Text {
                            content: "L".to_string(),
                            position: iced::Point::new(clip_x + clip_w - 14.0, clip_y + 3.0),
                            color: theme::ACCENT,
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clip == Some(clip.clip_id);
                let border_color = if is_selected {
                    theme::ACCENT
                } else {
                    clip_border_color
                };
                let border_width = if is_selected { 2.0 } else { 1.0 };

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(border_width),
                );

                // Clip name label
                if clip_w > 40.0 {
                    frame.fill_text(canvas::Text {
                        content: clip.name.clone(),
                        position: iced::Point::new(clip_x + 4.0, clip_y + 3.0),
                        color: theme::TEXT,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }
            }
        }

        // Draw note clips (for instrument tracks)
        if self.is_instrument && self.bpm > 0.0 {
            let note_clip_color = theme::with_alpha(self.track_color, 0.4);
            let note_block_color = theme::with_alpha(self.track_color, 0.8);

            for note_clip in &self.note_clips {
                let clip_x = self.beat_to_x(note_clip.position_beats);
                let clip_w = (note_clip.duration_beats * ppb as f64) as f32;

                // Skip clips outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = 4.0;
                let clip_h = h - 8.0;

                // Clip body
                frame.fill_rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                    note_clip_color,
                );

                // Draw note blocks inside the clip
                if !note_clip.notes.is_empty() && clip_w > 4.0 {
                    let pitches: Vec<u8> = note_clip.notes.iter().map(|n| n.0).collect();
                    let min_pitch = *pitches.iter().min().unwrap_or(&60);
                    let max_pitch = *pitches.iter().max().unwrap_or(&72);
                    let pitch_range = (max_pitch - min_pitch + 1).max(12) as f32;

                    for &(pitch, start_beat, duration_beats) in &note_clip.notes {
                        // start_beat is clip-local (0.0 = clip start)
                        let note_x =
                            clip_x + (start_beat / note_clip.duration_beats * clip_w as f64) as f32;
                        let note_w =
                            (duration_beats / note_clip.duration_beats * clip_w as f64) as f32;
                        let note_y_frac = (max_pitch.saturating_sub(pitch)) as f32 / pitch_range;
                        let note_y = clip_y + 2.0 + note_y_frac * (clip_h - 6.0);
                        let note_h = ((clip_h - 6.0) / pitch_range).clamp(2.0, 6.0);

                        frame.fill_rectangle(
                            iced::Point::new(note_x, note_y),
                            iced::Size::new(note_w.max(2.0), note_h),
                            note_block_color,
                        );
                    }
                }

                // Loop markers for note clips
                if note_clip.loop_enabled && note_clip.loop_end_beats > note_clip.loop_start_beats {
                    let loop_len = note_clip.loop_end_beats - note_clip.loop_start_beats;
                    let repeat_color = theme::with_alpha(self.track_color, 0.2);
                    let ghost_block_color = theme::with_alpha(self.track_color, 0.5);
                    let mut repeat_beat = note_clip.position_beats + note_clip.loop_end_beats;
                    while repeat_beat < note_clip.position_beats + note_clip.duration_beats {
                        let rx = self.beat_to_x(repeat_beat);
                        let rw = (loop_len * ppb as f64) as f32;
                        if rx < w && rx + rw > 0.0 {
                            frame.fill_rectangle(
                                iced::Point::new(rx, clip_y),
                                iced::Size::new(rw.min(clip_x + clip_w - rx).max(0.0), clip_h),
                                repeat_color,
                            );
                        }

                        // Draw ghost note blocks in this repeat
                        if !note_clip.notes.is_empty() && clip_w > 4.0 {
                            let pitches: Vec<u8> = note_clip.notes.iter().map(|n| n.0).collect();
                            let gmin = *pitches.iter().min().unwrap_or(&60);
                            let gmax = *pitches.iter().max().unwrap_or(&72);
                            let gpitch_range = (gmax - gmin + 1).max(12) as f32;
                            let offset = repeat_beat - note_clip.position_beats;

                            for &(pitch, start_beat, duration_beats) in &note_clip.notes {
                                // Only repeat notes within the loop region
                                if start_beat < note_clip.loop_start_beats
                                    || start_beat >= note_clip.loop_end_beats
                                {
                                    continue;
                                }
                                let gx = clip_x
                                    + ((start_beat + offset) / note_clip.duration_beats
                                        * clip_w as f64)
                                        as f32;
                                let gnw = (duration_beats / note_clip.duration_beats
                                    * clip_w as f64)
                                    as f32;
                                let gy_frac = (gmax.saturating_sub(pitch)) as f32 / gpitch_range;
                                let gy = clip_y + 2.0 + gy_frac * (clip_h - 6.0);
                                let gnh = ((clip_h - 6.0) / gpitch_range).clamp(2.0, 6.0);

                                if gx < clip_x + clip_w && gx + gnw > 0.0 {
                                    frame.fill_rectangle(
                                        iced::Point::new(gx, gy),
                                        iced::Size::new(gnw.max(2.0), gnh),
                                        ghost_block_color,
                                    );
                                }
                            }
                        }

                        repeat_beat += loop_len;
                    }

                    if clip_w > 20.0 {
                        frame.fill_text(canvas::Text {
                            content: "L".to_string(),
                            position: iced::Point::new(clip_x + clip_w - 14.0, clip_y + 3.0),
                            color: theme::ACCENT,
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clip == Some(note_clip.clip_id);
                let border_color = if is_selected {
                    theme::ACCENT
                } else {
                    theme::darken(self.track_color, 0.7)
                };
                let border_width = if is_selected { 2.0 } else { 1.0 };

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(border_width),
                );

                // Clip name label
                if clip_w > 40.0 {
                    frame.fill_text(canvas::Text {
                        content: note_clip.name.clone(),
                        position: iced::Point::new(clip_x + 4.0, clip_y + 3.0),
                        color: theme::TEXT,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }
            }
        }

        // Playhead overlay
        let playhead_x = self.beat_to_x(self.playhead_beats);
        if playhead_x >= 0.0 && playhead_x <= w {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, h),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

        // Bottom separator
        let sep = canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(theme::DIVIDER)
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(ref drag) = state.drag {
            return match drag {
                ClipDragAction::MoveClip { .. } => mouse::Interaction::Grabbing,
                ClipDragAction::ResizeClip { .. } => mouse::Interaction::ResizingHorizontally,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            if let Some((_, _, near_right, _, _)) = self.hit_test(pos.x) {
                if near_right {
                    return mouse::Interaction::ResizingHorizontally;
                }
                return mouse::Interaction::Grab;
            }
            return mouse::Interaction::Pointer;
        }

        mouse::Interaction::default()
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let track_id = self.track_id;

        match event {
            // -- Left click: select clip, start drag, or seek --
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if let Some((clip_id, is_note_clip, near_right, pos_beats, dur_beats)) =
                        self.hit_test(pos.x)
                    {
                        // Build selection message
                        let selection = if is_note_clip {
                            ArrangementSelection::NoteClip { track_id, clip_id }
                        } else {
                            ArrangementSelection::AudioClip { track_id, clip_id }
                        };

                        // Alt+click = split
                        // (iced canvas doesn't expose modifiers directly,
                        //  so we use keyboard events for alt; for now, right-edge = resize)

                        if near_right {
                            // Start resize drag
                            state.drag = Some(ClipDragAction::ResizeClip {
                                clip_id,
                                is_note_clip,
                                start_x: pos.x,
                                original_duration_beats: dur_beats,
                            });
                        } else {
                            // Start move drag
                            let click_beat = self.x_to_beat(pos.x);
                            state.drag = Some(ClipDragAction::MoveClip {
                                clip_id,
                                is_note_clip,
                                start_beat: click_beat,
                                original_position_beats: pos_beats,
                                start_y: pos.y,
                            });
                        }

                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SelectArrangementClip(selection)),
                        );
                    }

                    // No clip hit — seek (preserve existing behavior) + select track
                    if bounds.width > 0.0 {
                        let ppb = self.pixels_per_beat();
                        let beat = pos.x as f64 / ppb as f64 + self.scroll_offset_beats;
                        if self.total_beats > 0.0 {
                            let normalized = (beat / self.total_beats).clamp(0.0, 1.0);
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::Seek(normalized)),
                            );
                        }
                    }
                }
            }

            // -- Drag: move or resize clip --
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        let ppb = self.pixels_per_beat();

                        match drag {
                            ClipDragAction::MoveClip {
                                clip_id,
                                is_note_clip,
                                start_beat,
                                original_position_beats,
                                start_y,
                            } => {
                                let current_beat = self.x_to_beat(local_x);
                                let delta_beats = current_beat - start_beat;
                                let new_pos = (original_position_beats + delta_beats).max(0.0);

                                // Snap to nearest beat
                                let snapped = (new_pos * 4.0).round() / 4.0;

                                // Check for cross-track drag
                                let local_y = pos.y - bounds.y;
                                let dy = local_y - start_y;
                                let track_height = 70.0_f32;

                                if dy.abs() > track_height * 0.6 {
                                    let track_offset = (dy / track_height).round() as i32;
                                    let target_idx = (self.track_index as i32 + track_offset)
                                        .clamp(0, self.total_tracks as i32 - 1)
                                        as usize;

                                    if target_idx != self.track_index
                                        && target_idx < self.track_ids.len()
                                    {
                                        let target_track = self.track_ids[target_idx];
                                        let target_is_instrument = self.track_kinds[target_idx];

                                        // Type compatibility: note clips to instrument tracks,
                                        // audio clips to audio tracks
                                        if *is_note_clip == target_is_instrument {
                                            return (
                                                canvas::event::Status::Captured,
                                                Some(Message::MoveClipToTrack {
                                                    source_track: track_id,
                                                    target_track,
                                                    clip_id: *clip_id,
                                                    is_note_clip: *is_note_clip,
                                                }),
                                            );
                                        }
                                    }
                                }

                                if *is_note_clip {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::MoveNoteClipPosition {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_position_beats: snapped,
                                        }),
                                    );
                                } else {
                                    let spb = self.spb();
                                    let new_sample_pos = (snapped * spb) as u64;
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::MoveAudioClip {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_position: new_sample_pos,
                                        }),
                                    );
                                }
                            }
                            ClipDragAction::ResizeClip {
                                clip_id,
                                is_note_clip,
                                start_x,
                                original_duration_beats,
                            } => {
                                let dx = local_x - start_x;
                                let delta_beats = dx as f64 / ppb as f64;
                                let new_dur = (original_duration_beats + delta_beats).max(0.25);
                                // Snap to quarter beat
                                let snapped = (new_dur * 4.0).round() / 4.0;

                                if *is_note_clip {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::ResizeNoteClipDuration {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_duration_beats: snapped,
                                        }),
                                    );
                                } else {
                                    let spb = self.spb();
                                    let new_dur_samples = (snapped * spb) as u64;
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::ResizeAudioClip {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_duration: new_dur_samples.max(1),
                                        }),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // -- Release: end drag --
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if state.drag.is_some() {
                    state.drag = None;
                    return (canvas::event::Status::Captured, None);
                }
            }

            // -- Scroll: zoom/pan (preserve existing) --
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let (dx, dy) = match delta {
                        iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                        iced::mouse::ScrollDelta::Pixels { x, y } => (x / 20.0, y / 20.0),
                    };
                    // Shift+scroll or horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ScrollArrangement(-dx as f64 * 2.0)),
                        );
                    }
                    // Vertical scroll for zoom
                    if dy > 0.0 {
                        return (canvas::event::Status::Captured, Some(Message::ZoomIn));
                    } else if dy < 0.0 {
                        return (canvas::event::Status::Captured, Some(Message::ZoomOut));
                    }
                }
            }

            // -- Keyboard: Delete/Backspace for selected clip --
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
                ..
            })
            | canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
                ..
            }) => {
                if self.selected_clip.is_some() {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::DeleteSelectedClip),
                    );
                }
            }

            // -- Keyboard: Ctrl+D for duplicate --
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character(ref c),
                modifiers,
                ..
            }) => {
                if modifiers.control() && c.as_str() == "d" && self.selected_clip.is_some() {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::DuplicateSelectedClip),
                    );
                }
            }

            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}
