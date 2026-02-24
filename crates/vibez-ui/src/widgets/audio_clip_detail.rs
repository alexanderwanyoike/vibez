use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use vibez_core::audio_buffer::DecodedAudio;

use crate::message::Message;
use crate::theme;

/// Canvas widget for showing a detailed waveform of an audio clip in the detail panel.
pub struct AudioClipDetailWidget {
    pub audio: Arc<DecodedAudio>,
    pub duration_samples: u64,
    pub source_offset: u64,
    pub sample_rate: u32,
    pub track_color: Color,
    /// Normalized playhead position within the clip (0.0..1.0), negative means not in clip.
    pub playhead_normalized: f64,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
}

impl canvas::Program<Message> for AudioClipDetailWidget {
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
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::BG_DARK);

        // Center line
        let center_y = h / 2.0;
        let center_line = canvas::Path::line(
            iced::Point::new(0.0, center_y),
            iced::Point::new(w, center_y),
        );
        frame.stroke(
            &center_line,
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.3,
                    ..theme::TEXT_DIM
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
            let half_h = h / 2.0 - 2.0;
            let channels = self.audio.num_channels();
            let waveform_color = theme::with_alpha(self.track_color, 0.7);
            let loop_line_color = theme::with_alpha(self.track_color, 0.35);

            // Draw loop boundary markers if looping
            if looping {
                let loop_start_px =
                    (loop_start - self.source_offset as usize) as f32 / num_frames as f32 * w;
                // The loop region repeats — show a subtle vertical line at each loop boundary
                let mut boundary = loop_end - self.source_offset as usize;
                while boundary < num_frames {
                    let bx = boundary as f32 / num_frames as f32 * w;
                    let line =
                        canvas::Path::line(iced::Point::new(bx, 0.0), iced::Point::new(bx, h));
                    frame.stroke(
                        &line,
                        canvas::Stroke::default()
                            .with_color(loop_line_color)
                            .with_width(1.0),
                    );
                    boundary += loop_len;
                }
                // Also draw the loop start marker
                let start_line = canvas::Path::line(
                    iced::Point::new(loop_start_px, 0.0),
                    iced::Point::new(loop_start_px, h),
                );
                frame.stroke(
                    &start_line,
                    canvas::Stroke::default()
                        .with_color(loop_line_color)
                        .with_width(1.0),
                );
            }

            for px in 0..pixels {
                // Map pixel to clip-local frame range
                let clip_frame_start = px * num_frames / pixels.max(1);
                let clip_frame_end = (px + 1) * num_frames / pixels.max(1);

                let mut min_val = 0.0f32;
                let mut max_val = 0.0f32;

                // Sample each clip frame in this pixel's range, resolving loop wrapping
                let sample_count = (clip_frame_end - clip_frame_start).max(1);
                for cf in clip_frame_start..clip_frame_end.max(clip_frame_start + 1) {
                    let source_frame = if looping {
                        let raw = self.source_offset as usize + cf;
                        if raw >= loop_end {
                            loop_start + (raw - loop_start) % loop_len
                        } else {
                            raw
                        }
                    } else {
                        self.source_offset as usize + cf
                    };

                    for ch in 0..channels {
                        let s = self.audio.sample(ch, source_frame);
                        min_val = min_val.min(s);
                        max_val = max_val.max(s);
                    }
                }

                // For wider pixel ranges, use peak_in_range for efficiency on non-looped sections
                if !looping && sample_count > 2 {
                    let range_start = self.source_offset as usize + clip_frame_start;
                    let range_end = self.source_offset as usize + clip_frame_end;
                    min_val = 0.0;
                    max_val = 0.0;
                    for ch in 0..channels {
                        let (ch_min, ch_max) = self.audio.peak_in_range(ch, range_start, range_end);
                        min_val += ch_min;
                        max_val += ch_max;
                    }
                    if channels > 0 {
                        min_val /= channels as f32;
                        max_val /= channels as f32;
                    }
                }

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

        // Playhead
        if self.playhead_normalized >= 0.0 && self.playhead_normalized <= 1.0 {
            let px = (self.playhead_normalized as f32) * w;
            let playhead_line =
                canvas::Path::line(iced::Point::new(px, 0.0), iced::Point::new(px, h));
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

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
}
