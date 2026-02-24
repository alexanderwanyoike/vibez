use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme;
use vibez_core::midi::TrackKind;

// ── Lightweight data types for rendering ──

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
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
    pub position_beats: f64,
    pub duration_beats: f64,
    pub name: String,
    pub notes: Vec<(u8, f64, f64)>, // (pitch, start_beat, duration_beats)
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// Compute waveform peaks for a clip.
pub fn compute_clip_peaks(clip: &crate::state::UiClip) -> Vec<(f32, f32)> {
    let num_peaks = (clip.duration as usize / 100).clamp(1, 1000);
    (0..num_peaks)
        .map(|i| {
            let start = clip.source_offset as usize + i * clip.duration as usize / num_peaks;
            let end = clip.source_offset as usize + (i + 1) * clip.duration as usize / num_peaks;
            let channels = clip.audio.num_channels();
            if channels == 0 {
                return (0.0, 0.0);
            }
            let mut min_val = 0.0f32;
            let mut max_val = 0.0f32;
            for ch in 0..channels {
                let (ch_min, ch_max) = clip.audio.peak_in_range(ch, start, end);
                min_val += ch_min;
                max_val += ch_max;
            }
            (min_val / channels as f32, max_val / channels as f32)
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

                    // Bar label
                    if ppb < 10.0 {
                        // Low zoom: bar numbers only
                        let label = format!("{}", bar_index + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 4.0, 3.0),
                            color: theme::RULER_TEXT,
                            size: iced::Pixels(12.0),
                            ..Default::default()
                        });
                    } else {
                        // Medium/high zoom: bar.beat
                        let label = format!("{}.1", bar_index + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 4.0, 3.0),
                            color: theme::RULER_TEXT,
                            size: iced::Pixels(12.0),
                            ..Default::default()
                        });
                    }
                } else {
                    // Beat line (thin)
                    let tick =
                        canvas::Path::line(iced::Point::new(x, h * 0.65), iced::Point::new(x, h));
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );

                    // Beat label at medium zoom
                    if ppb >= 10.0 {
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

                // Sub-beat ticks at high zoom
                if ppb > 40.0 {
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

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
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
    ) -> Self {
        let clips = track
            .clips
            .iter()
            .map(|c| TimelineClip {
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
}

impl canvas::Program<Message> for TrackClipCanvas {
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
        let ppb = self.pixels_per_beat();

        // Background
        let bg_color = if self.selected {
            theme::TRACK_BG_SELECTED
        } else {
            theme::TRACK_BG
        };
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), bg_color);

        // Grid lines (only visible beats)
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
                let line_color = if is_bar {
                    theme::BORDER
                } else {
                    theme::DIVIDER
                };
                let vline = canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                frame.stroke(
                    &vline,
                    canvas::Stroke::default()
                        .with_color(line_color)
                        .with_width(if is_bar { 1.0 } else { 0.5 }),
                );
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

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(clip_border_color)
                        .with_width(1.0),
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
                        let note_x = clip_x
                            + ((start_beat - note_clip.position_beats) / note_clip.duration_beats
                                * clip_w as f64) as f32;
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

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(theme::darken(self.track_color, 0.7))
                        .with_width(1.0),
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
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
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
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}
