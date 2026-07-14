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
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::bg_dark());

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
            let half_h = h / 2.0 - 2.0;
            let channels = self.audio.num_channels();
            let waveform_color = theme::with_alpha(self.track_color, 0.7);
            let loop_line_color = theme::with_alpha(self.track_color, 0.35);

            // Draw loop boundary markers if looping
            if looping {
                // The clip's source offset can sit past the loop points
                // (e.g. a trimmed clip start); saturate so markers clamp
                // to the left edge instead of underflowing.
                let source_offset = self.source_offset as usize;
                let loop_start_px =
                    loop_start.saturating_sub(source_offset) as f32 / num_frames as f32 * w;
                // The loop region repeats — show a subtle vertical line at each loop boundary
                let mut boundary = loop_end.saturating_sub(source_offset);
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
