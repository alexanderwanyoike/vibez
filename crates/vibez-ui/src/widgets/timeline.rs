use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme;

// ── Lightweight data types for rendering ──

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
    pub position: u64,
    pub duration: u64,
    pub name: String,
    /// Pre-computed waveform peaks for mini display (per pixel column).
    pub peaks: Vec<(f32, f32)>,
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

/// Canvas widget that draws the time ruler ticks/labels + playhead line.
pub struct RulerWidget {
    pub playhead_position: f64,
    pub duration_seconds: f64,
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

        // Ruler tick marks and labels
        if self.duration_seconds > 0.0 {
            let seconds_per_pixel = self.duration_seconds / w as f64;
            let tick_interval = tick_interval_for_duration(self.duration_seconds);

            let mut t = 0.0;
            while t <= self.duration_seconds {
                let x = (t / self.duration_seconds * w as f64) as f32;
                let tick = canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::RULER_LINE)
                        .with_width(1.0),
                );

                let label = format_ruler_time(t);
                frame.fill_text(canvas::Text {
                    content: label,
                    position: iced::Point::new(x + 3.0, 4.0),
                    color: theme::RULER_TEXT,
                    size: iced::Pixels(10.0),
                    ..Default::default()
                });

                t += tick_interval;
                if seconds_per_pixel > 1000.0 {
                    break;
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
                    .with_width(1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}

// ── TrackClipCanvas ──

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
    pub clips: Vec<TimelineClip>,
    pub playhead_position: f64,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub selected: bool,
}

impl TrackClipCanvas {
    pub fn from_track(
        track: &UiTrack,
        playhead_position: f64,
        duration_seconds: f64,
        sample_rate: u32,
        selected: bool,
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
        Self {
            clips,
            playhead_position,
            duration_seconds,
            sample_rate,
            selected,
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

        // Draw clips
        if self.duration_seconds > 0.0 {
            let total_samples = self.duration_seconds * self.sample_rate as f64;

            for clip in &self.clips {
                let clip_x = (clip.position as f64 / total_samples * w as f64) as f32;
                let clip_w = (clip.duration as f64 / total_samples * w as f64) as f32;
                let clip_y = 4.0;
                let clip_h = h - 8.0;

                // Clip body
                frame.fill_rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                    theme::CLIP_BODY,
                );

                // Mini waveform
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
                            Color {
                                a: 0.7,
                                ..theme::WAVEFORM
                            },
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
                        .with_color(theme::CLIP_BORDER)
                        .with_width(1.0),
                );

                // Clip name label
                if clip_w > 40.0 {
                    frame.fill_text(canvas::Text {
                        content: clip.name.clone(),
                        position: iced::Point::new(clip_x + 4.0, clip_y + 3.0),
                        color: theme::TEXT,
                        size: iced::Pixels(10.0),
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
                    .with_width(1.5),
            );
        }

        // Bottom separator
        let sep = canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.3,
                    ..theme::TEXT_DIM
                })
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

/// Choose an appropriate tick interval based on total duration.
fn tick_interval_for_duration(duration: f64) -> f64 {
    if duration <= 5.0 {
        0.5
    } else if duration <= 15.0 {
        1.0
    } else if duration <= 60.0 {
        5.0
    } else if duration <= 300.0 {
        15.0
    } else {
        60.0
    }
}

/// Format time for ruler labels.
fn format_ruler_time(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u32;
    let secs = seconds % 60.0;
    if mins > 0 {
        format!("{mins}:{secs:04.1}")
    } else {
        format!("{secs:.1}s")
    }
}
