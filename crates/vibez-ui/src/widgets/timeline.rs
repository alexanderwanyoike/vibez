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
}

/// Lightweight copy of a note clip for timeline rendering.
pub struct TimelineNoteClip {
    pub position_beats: f64,
    pub duration_beats: f64,
    pub name: String,
    pub notes: Vec<(u8, f64, f64)>, // (pitch, start_beat, duration_beats)
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

/// Canvas widget that draws a beat-based ruler with playhead.
pub struct RulerWidget {
    pub playhead_position: f64,
    pub duration_seconds: f64,
    pub bpm: f64,
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

        // Beat-based ruler
        if self.duration_seconds > 0.0 && self.bpm > 0.0 {
            let total_beats = self.duration_seconds * self.bpm / 60.0;
            let beats_per_bar = 4.0;
            let total_bars = (total_beats / beats_per_bar).ceil() as usize;

            for bar in 0..=total_bars {
                let beat = bar as f64 * beats_per_bar;
                let x = (beat / total_beats * w as f64) as f32;

                if x > w {
                    break;
                }

                // Bar line
                let tick = canvas::Path::line(iced::Point::new(x, h * 0.4), iced::Point::new(x, h));
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::BORDER)
                        .with_width(1.0),
                );

                // Bar number label
                let label = format!("{}", bar + 1);
                frame.fill_text(canvas::Text {
                    content: label,
                    position: iced::Point::new(x + 4.0, 3.0),
                    color: theme::RULER_TEXT,
                    size: iced::Pixels(12.0),
                    ..Default::default()
                });

                // Sub-beat ticks
                for sub_beat in 1..4 {
                    let sub_x = ((beat + sub_beat as f64) / total_beats * w as f64) as f32;
                    if sub_x > w {
                        break;
                    }
                    let sub_tick = canvas::Path::line(
                        iced::Point::new(sub_x, h * 0.65),
                        iced::Point::new(sub_x, h),
                    );
                    frame.stroke(
                        &sub_tick,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );
                }
            }
        }

        // Playhead
        if self.duration_seconds > 0.0 {
            let x = (self.playhead_position as f32) * w;
            let playhead_line =
                canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

        vec![frame.into_geometry()]
    }
}

// ── TrackClipCanvas ──

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
    pub clips: Vec<TimelineClip>,
    pub note_clips: Vec<TimelineNoteClip>,
    pub playhead_position: f64,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub selected: bool,
    pub track_color: Color,
    pub is_instrument: bool,
    pub bpm: f64,
}

impl TrackClipCanvas {
    pub fn from_track(
        track: &UiTrack,
        playhead_position: f64,
        duration_seconds: f64,
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
            })
            .collect();
        Self {
            clips,
            note_clips,
            playhead_position,
            duration_seconds,
            sample_rate,
            selected,
            track_color,
            is_instrument: matches!(track.kind, TrackKind::Instrument(_)),
            bpm,
        }
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

        // Background
        let bg_color = if self.selected {
            theme::TRACK_BG_SELECTED
        } else {
            theme::TRACK_BG
        };
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), bg_color);

        // Grid lines
        if self.duration_seconds > 0.0 && self.bpm > 0.0 {
            let total_beats = self.duration_seconds * self.bpm / 60.0;
            let num_beats = total_beats.ceil() as usize;
            for beat in 0..=num_beats {
                let x = (beat as f64 / total_beats * w as f64) as f32;
                if x > w {
                    break;
                }
                let is_bar = beat % 4 == 0;
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
        if self.duration_seconds > 0.0 {
            let total_samples = self.duration_seconds * self.sample_rate as f64;
            let clip_color = theme::with_alpha(self.track_color, 0.5);
            let clip_border_color = theme::darken(self.track_color, 0.7);
            let waveform_color = theme::with_alpha(self.track_color, 0.6);

            for clip in &self.clips {
                let clip_x = (clip.position as f64 / total_samples * w as f64) as f32;
                let clip_w = (clip.duration as f64 / total_samples * w as f64) as f32;
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
                        let peak_idx = px * clip.peaks.len() / pixels.max(1);
                        if peak_idx >= clip.peaks.len() {
                            break;
                        }
                        let (min_val, max_val) = clip.peaks[peak_idx];
                        let y_top = center_y - (max_val * half_h);
                        let y_bottom = center_y - (min_val * half_h);
                        let height = (y_bottom - y_top).max(1.0);
                        frame.fill_rectangle(
                            iced::Point::new(clip_x + px as f32, y_top),
                            iced::Size::new(1.0, height),
                            waveform_color,
                        );
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
        if self.is_instrument && self.duration_seconds > 0.0 && self.bpm > 0.0 {
            let total_beats = self.duration_seconds * self.bpm / 60.0;
            let note_clip_color = theme::with_alpha(self.track_color, 0.4);
            let note_block_color = theme::with_alpha(self.track_color, 0.8);

            for note_clip in &self.note_clips {
                let clip_x = (note_clip.position_beats / total_beats * w as f64) as f32;
                let clip_w = (note_clip.duration_beats / total_beats * w as f64) as f32;
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
                    // Find pitch range for scaling
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
        if self.duration_seconds > 0.0 {
            let x = (self.playhead_position as f32) * w;
            let playhead_line =
                canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
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
        if let canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) =
            event
        {
            if let Some(pos) = cursor.position_in(bounds) {
                if bounds.width > 0.0 {
                    let normalized = (pos.x / bounds.width) as f64;
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Seek(normalized.clamp(0.0, 1.0))),
                    );
                }
            }
        }

        (canvas::event::Status::Ignored, None)
    }
}
