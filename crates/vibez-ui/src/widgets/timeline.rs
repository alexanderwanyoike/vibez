use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme;

/// Height of each track lane in pixels.
const TRACK_LANE_HEIGHT: f32 = 80.0;
/// Width of the track header area.
const TRACK_HEADER_WIDTH: f32 = 120.0;
/// Height of the time ruler at the top.
const TIME_RULER_HEIGHT: f32 = 24.0;

pub struct TimelineWidget {
    /// Snapshot of track data for rendering.
    pub tracks: Vec<TimelineTrack>,
    /// Playhead position 0.0..1.0.
    pub playhead_position: f64,
    /// Total duration in seconds (for ruler).
    pub duration_seconds: f64,
    /// Currently selected track index.
    pub selected_track_idx: Option<usize>,
    pub sample_rate: u32,
    pub cache: canvas::Cache,
}

/// Lightweight copy of track data for rendering.
pub struct TimelineTrack {
    pub name: String,
    pub clips: Vec<TimelineClip>,
    pub mute: bool,
    pub solo: bool,
}

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
    pub position: u64,
    pub duration: u64,
    pub name: String,
    /// Pre-computed waveform peaks for mini display (per pixel column).
    pub peaks: Vec<(f32, f32)>,
}

impl Default for TimelineWidget {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            playhead_position: 0.0,
            duration_seconds: 0.0,
            selected_track_idx: None,
            sample_rate: 44_100,
            cache: canvas::Cache::new(),
        }
    }
}

impl TimelineWidget {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the timeline from UI state.
    pub fn sync_from_tracks(
        &mut self,
        tracks: &[UiTrack],
        selected_idx: Option<usize>,
        duration_seconds: f64,
        sample_rate: u32,
    ) {
        self.selected_track_idx = selected_idx;
        self.duration_seconds = duration_seconds;
        self.sample_rate = sample_rate;

        // Only rebuild if track/clip structure changed
        let needs_rebuild = self.tracks.len() != tracks.len()
            || self
                .tracks
                .iter()
                .zip(tracks.iter())
                .any(|(old, new)| old.clips.len() != new.clips.len() || old.name != new.name);

        if needs_rebuild {
            self.tracks = tracks
                .iter()
                .map(|t| TimelineTrack {
                    name: t.name.clone(),
                    clips: t
                        .clips
                        .iter()
                        .map(|c| {
                            // Compute mini-waveform peaks (simplified: 1 peak per ~100 samples)
                            let num_peaks = (c.duration as usize / 100).clamp(1, 1000);
                            let peaks: Vec<(f32, f32)> = (0..num_peaks)
                                .map(|i| {
                                    let start = c.source_offset as usize
                                        + i * c.duration as usize / num_peaks;
                                    let end = c.source_offset as usize
                                        + (i + 1) * c.duration as usize / num_peaks;
                                    let channels = c.audio.num_channels();
                                    if channels == 0 {
                                        return (0.0, 0.0);
                                    }
                                    let mut min_val = 0.0f32;
                                    let mut max_val = 0.0f32;
                                    for ch in 0..channels {
                                        let (ch_min, ch_max) =
                                            c.audio.peak_in_range(ch, start, end);
                                        min_val += ch_min;
                                        max_val += ch_max;
                                    }
                                    (min_val / channels as f32, max_val / channels as f32)
                                })
                                .collect();

                            TimelineClip {
                                position: c.position,
                                duration: c.duration,
                                name: c.name.clone(),
                                peaks,
                            }
                        })
                        .collect(),
                    mute: t.mute,
                    solo: t.solo,
                })
                .collect();
            self.cache.clear();
        }
    }

    pub fn set_playhead(&mut self, pos: f64) {
        self.playhead_position = pos;
    }
}

impl canvas::Program<Message> for TimelineWidget {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let bg = self.cache.draw(renderer, bounds.size(), |frame| {
            let w = bounds.width;
            let h = bounds.height;
            let content_w = w - TRACK_HEADER_WIDTH;

            // Background
            frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::BG_DARK);

            // Time ruler
            frame.fill_rectangle(
                iced::Point::new(TRACK_HEADER_WIDTH, 0.0),
                iced::Size::new(content_w, TIME_RULER_HEIGHT),
                theme::RULER_BG,
            );

            // Ruler tick marks and labels
            if self.duration_seconds > 0.0 {
                let seconds_per_pixel = self.duration_seconds / content_w as f64;
                let tick_interval = tick_interval_for_duration(self.duration_seconds);

                let mut t = 0.0;
                while t <= self.duration_seconds {
                    let x =
                        TRACK_HEADER_WIDTH + (t / self.duration_seconds * content_w as f64) as f32;
                    let tick = canvas::Path::line(
                        iced::Point::new(x, 0.0),
                        iced::Point::new(x, TIME_RULER_HEIGHT),
                    );
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::RULER_LINE)
                            .with_width(1.0),
                    );

                    // Label
                    let label = format_ruler_time(t);
                    frame.fill_text(canvas::Text {
                        content: label,
                        position: iced::Point::new(x + 3.0, 4.0),
                        color: theme::RULER_TEXT,
                        size: iced::Pixels(10.0),
                        ..Default::default()
                    });

                    t += tick_interval;
                    // Safety: prevent infinite loop for very small intervals
                    if seconds_per_pixel > 1000.0 {
                        break;
                    }
                }
            }

            // Draw track lanes
            for (i, track) in self.tracks.iter().enumerate() {
                let lane_y = TIME_RULER_HEIGHT + i as f32 * TRACK_LANE_HEIGHT;

                if lane_y > h {
                    break;
                }

                // Track lane background
                let bg_color = if self.selected_track_idx == Some(i) {
                    theme::TRACK_BG_SELECTED
                } else {
                    theme::TRACK_BG
                };
                frame.fill_rectangle(
                    iced::Point::new(0.0, lane_y),
                    iced::Size::new(w, TRACK_LANE_HEIGHT),
                    bg_color,
                );

                // Lane separator line
                let sep = canvas::Path::line(
                    iced::Point::new(0.0, lane_y + TRACK_LANE_HEIGHT),
                    iced::Point::new(w, lane_y + TRACK_LANE_HEIGHT),
                );
                frame.stroke(
                    &sep,
                    canvas::Stroke::default()
                        .with_color(Color {
                            a: 0.3,
                            ..theme::TEXT_DIM
                        })
                        .with_width(1.0),
                );

                // Track header
                frame.fill_rectangle(
                    iced::Point::new(0.0, lane_y),
                    iced::Size::new(TRACK_HEADER_WIDTH, TRACK_LANE_HEIGHT),
                    theme::BG_SURFACE,
                );

                // Track name
                let name_color = if track.mute {
                    theme::TEXT_DIM
                } else {
                    theme::TEXT
                };
                frame.fill_text(canvas::Text {
                    content: track.name.clone(),
                    position: iced::Point::new(8.0, lane_y + 8.0),
                    color: name_color,
                    size: iced::Pixels(12.0),
                    ..Default::default()
                });

                // Mute/Solo indicators
                if track.mute {
                    frame.fill_text(canvas::Text {
                        content: "M".to_string(),
                        position: iced::Point::new(8.0, lane_y + 28.0),
                        color: theme::MUTE_ACTIVE,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }
                if track.solo {
                    frame.fill_text(canvas::Text {
                        content: "S".to_string(),
                        position: iced::Point::new(22.0, lane_y + 28.0),
                        color: theme::SOLO_ACTIVE,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }

                // Draw clips
                if self.duration_seconds > 0.0 {
                    let total_samples = self.duration_seconds * self.sample_rate as f64;

                    for clip in &track.clips {
                        let clip_x = TRACK_HEADER_WIDTH
                            + (clip.position as f64 / total_samples * content_w as f64) as f32;
                        let clip_w =
                            (clip.duration as f64 / total_samples * content_w as f64) as f32;
                        let clip_y = lane_y + 4.0;
                        let clip_h = TRACK_LANE_HEIGHT - 8.0;

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
            }

            // Header/content separator
            let header_sep = canvas::Path::line(
                iced::Point::new(TRACK_HEADER_WIDTH, 0.0),
                iced::Point::new(TRACK_HEADER_WIDTH, h),
            );
            frame.stroke(
                &header_sep,
                canvas::Stroke::default()
                    .with_color(Color {
                        a: 0.4,
                        ..theme::TEXT_DIM
                    })
                    .with_width(1.0),
            );
        });

        // Playhead (drawn every frame, not cached)
        let playhead = {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            let content_w = bounds.width - TRACK_HEADER_WIDTH;
            if self.duration_seconds > 0.0 {
                let x = TRACK_HEADER_WIDTH + (self.playhead_position as f32) * content_w;
                let playhead_line = canvas::Path::line(
                    iced::Point::new(x, 0.0),
                    iced::Point::new(x, bounds.height),
                );
                frame.stroke(
                    &playhead_line,
                    canvas::Stroke::default()
                        .with_color(theme::PLAYHEAD)
                        .with_width(1.5),
                );
            }
            frame.into_geometry()
        };

        vec![bg, playhead]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(pos) = cursor.position_in(bounds) {
            if pos.x > TRACK_HEADER_WIDTH && pos.y > TIME_RULER_HEIGHT {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
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
                let content_w = bounds.width - TRACK_HEADER_WIDTH;

                // Click on timeline content area → seek
                if pos.x > TRACK_HEADER_WIDTH && pos.y > TIME_RULER_HEIGHT && content_w > 0.0 {
                    let normalized = ((pos.x - TRACK_HEADER_WIDTH) / content_w) as f64;
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::Seek(normalized.clamp(0.0, 1.0))),
                    );
                }

                // Click on track header → select track
                if pos.x <= TRACK_HEADER_WIDTH && pos.y > TIME_RULER_HEIGHT {
                    let track_idx = ((pos.y - TIME_RULER_HEIGHT) / TRACK_LANE_HEIGHT) as usize;
                    if track_idx < self.tracks.len() {
                        // We need the TrackId but we don't store it here.
                        // This is handled by mapping the index in the app.
                        // For now, return None and let the app handle clicks via container.
                    }
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
