//! Drawing half of the clip lane canvas.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer};

use crate::theme;

use super::*;

fn fit_clip_title(name: &str, clip_width: f32, loop_icon_visible: bool) -> Option<String> {
    if clip_width <= 40.0 {
        return None;
    }
    const HORIZONTAL_PADDING: f32 = 8.0;
    const LOOP_ICON_WIDTH: f32 = 18.0;
    const APPROX_GLYPH_WIDTH: f32 = 6.5;
    let reserved = HORIZONTAL_PADDING
        + if loop_icon_visible {
            LOOP_ICON_WIDTH
        } else {
            0.0
        };
    let max_chars = ((clip_width - reserved).max(0.0) / APPROX_GLYPH_WIDTH).floor() as usize;
    if max_chars < 4 {
        return None;
    }
    if name.chars().count() <= max_chars {
        Some(name.to_string())
    } else {
        let prefix: String = name.chars().take(max_chars - 2).collect();
        Some(format!("{prefix}.."))
    }
}

fn visible_pixel_columns(
    clip_x: f32,
    clip_width: f32,
    viewport_width: f32,
) -> std::ops::Range<usize> {
    let pixels = clip_width.max(0.0) as usize;
    let start = (-clip_x).ceil().max(0.0) as usize;
    let end = (viewport_width - clip_x).ceil().max(0.0) as usize;
    let start = start.min(pixels);
    let end = end.min(pixels);
    if start < end {
        start..end
    } else {
        0..0
    }
}

fn visible_title_bounds(
    clip_x: f32,
    title_width: f32,
    viewport_width: f32,
) -> Option<(f32, f32, f32)> {
    let left = clip_x.max(0.0);
    let right = (clip_x + title_width).min(viewport_width);
    (right > left).then(|| (left, right - left, (clip_x + 4.0).max(left + 4.0)))
}

impl TrackClipCanvas {
    pub(super) fn draw_impl(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let ppb = self.pixels_per_beat();

        // True when a sample drag is active and the cursor is hovering this
        // lane. Used to paint a drop indicator.
        let drop_hover = self.sample_drop_active && cursor.position_in(bounds).is_some();
        let drop_compatible = !self.is_instrument;

        // Background
        let bg_color = if drop_hover && drop_compatible {
            theme::accent_dim()
        } else if drop_hover {
            theme::with_alpha(theme::danger(), 0.16)
        } else if self.selected {
            theme::track_bg_selected()
        } else {
            theme::track_bg()
        };
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), bg_color);

        // Grid lines use the same effective division as interaction.
        if self.bpm > 0.0 {
            let visible = w as f64 / ppb as f64;
            let grid = self.grid.effective_grid(ppb);
            let step = grid.beat_size();
            let start = (self.scroll_offset_beats / step).floor().max(0.0) as i64;
            let end = ((self.scroll_offset_beats + visible) / step).ceil() as i64 + 1;

            for grid_i in start..end {
                let beat = grid_i as f64 * step;
                let x = self.beat_to_x(beat);
                if x < -1.0 || x > w + 1.0 {
                    continue;
                }
                let on_bar = (beat / 4.0 - (beat / 4.0).round()).abs() < 1e-6;
                let on_beat = (beat - beat.round()).abs() < 1e-6;
                let (color, width) = if on_bar {
                    (theme::grid_bar(), 1.0)
                } else if on_beat {
                    (theme::grid_beat(), 0.75)
                } else {
                    (theme::grid_sub(), 0.5)
                };
                let line = canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                frame.stroke(
                    &line,
                    canvas::Stroke::default()
                        .with_color(color)
                        .with_width(width),
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

                // Mini waveform using track color (drawn below title bar)
                if !clip.peaks.is_empty() && clip_w > 4.0 {
                    let body_top = clip_y + CLIP_TITLE_HEIGHT;
                    let body_h = clip_h - CLIP_TITLE_HEIGHT;
                    let center_y = body_top + body_h / 2.0;
                    let half_h = body_h / 2.0 - 2.0;
                    let pixels = clip_w as usize;
                    for px in visible_pixel_columns(clip_x, clip_w, w) {
                        let screen_x = clip_x + px as f32;
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
                            color: theme::accent(),
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clips.contains(&clip.clip_id);
                let border_color = if is_selected {
                    theme::accent()
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

                // Title bar separator
                let title_sep_y = clip_y + CLIP_TITLE_HEIGHT;
                let title_line = canvas::Path::line(
                    iced::Point::new(clip_x, title_sep_y),
                    iced::Point::new(clip_x + clip_w.max(2.0), title_sep_y),
                );
                frame.stroke(
                    &title_line,
                    canvas::Stroke::default()
                        .with_color(theme::with_alpha(Color::BLACK, 0.3))
                        .with_width(1.0),
                );

                // Clip name label
                let title_width = (clip_w - if clip.loop_enabled { 18.0 } else { 0.0 }).max(0.0);
                if let Some((title_x, visible_title_width, text_x)) =
                    visible_title_bounds(clip_x, title_width, w)
                {
                    if let Some(title) =
                        fit_clip_title(clip.name.as_str(), visible_title_width, clip.loop_enabled)
                    {
                        frame.with_clip(
                            Rectangle {
                                x: title_x,
                                y: clip_y,
                                width: visible_title_width,
                                height: CLIP_TITLE_HEIGHT,
                            },
                            |title_frame| {
                                title_frame.fill_text(canvas::Text {
                                    content: title,
                                    position: iced::Point::new(text_x, clip_y + 3.0),
                                    color: theme::text(),
                                    size: iced::Pixels(11.0),
                                    ..Default::default()
                                });
                            },
                        );
                    }
                }

                // Diagonal stripe overlay when the clip's warp is
                // stale relative to the current project tempo. Low
                // opacity so the waveform and clip colour remain
                // legible, but strong enough to catch the eye.
                if clip.warp_stale && clip_w > 4.0 {
                    let stripe_color = theme::with_alpha(theme::meter_yellow(), 0.22);
                    let stripe_spacing = 8.0f32;
                    let stripe_stroke = 1.5f32;
                    let mut offset = -clip_h;
                    while offset < clip_w {
                        let x0 = clip_x + offset;
                        let x1 = clip_x + offset + clip_h;
                        let a_x = x0.clamp(clip_x, clip_x + clip_w);
                        let b_x = x1.clamp(clip_x, clip_x + clip_w);
                        let a_t = if (x1 - x0).abs() > 0.1 {
                            (a_x - x0) / (x1 - x0)
                        } else {
                            0.0
                        };
                        let b_t = if (x1 - x0).abs() > 0.1 {
                            (b_x - x0) / (x1 - x0)
                        } else {
                            0.0
                        };
                        let a_y = clip_y + a_t * clip_h;
                        let b_y = clip_y + b_t * clip_h;
                        if (b_x - a_x).abs() > 0.5 {
                            let line = canvas::Path::line(
                                iced::Point::new(a_x, a_y),
                                iced::Point::new(b_x, b_y),
                            );
                            frame.stroke(
                                &line,
                                canvas::Stroke::default()
                                    .with_color(stripe_color)
                                    .with_width(stripe_stroke),
                            );
                        }
                        offset += stripe_spacing;
                    }
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

                // Draw note blocks inside the clip (below title bar)
                if !note_clip.notes.is_empty() && clip_w > 4.0 {
                    let body_top = clip_y + CLIP_TITLE_HEIGHT;
                    let body_h = clip_h - CLIP_TITLE_HEIGHT;
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
                        let note_y = body_top + 2.0 + note_y_frac * (body_h - 4.0);
                        let note_h = ((body_h - 4.0) / pitch_range).clamp(2.0, 6.0);

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

                        // Draw ghost note blocks in this repeat (below title bar)
                        if !note_clip.notes.is_empty() && clip_w > 4.0 {
                            let gbody_top = clip_y + CLIP_TITLE_HEIGHT;
                            let gbody_h = clip_h - CLIP_TITLE_HEIGHT;
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
                                let gy = gbody_top + 2.0 + gy_frac * (gbody_h - 4.0);
                                let gnh = ((gbody_h - 4.0) / gpitch_range).clamp(2.0, 6.0);

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
                            color: theme::accent(),
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clips.contains(&note_clip.clip_id);
                let border_color = if is_selected {
                    theme::accent()
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

                // Title bar separator
                let title_sep_y = clip_y + CLIP_TITLE_HEIGHT;
                let title_line = canvas::Path::line(
                    iced::Point::new(clip_x, title_sep_y),
                    iced::Point::new(clip_x + clip_w.max(2.0), title_sep_y),
                );
                frame.stroke(
                    &title_line,
                    canvas::Stroke::default()
                        .with_color(theme::with_alpha(Color::BLACK, 0.3))
                        .with_width(1.0),
                );

                // Clip name label
                let title_width =
                    (clip_w - if note_clip.loop_enabled { 18.0 } else { 0.0 }).max(0.0);
                if let Some((title_x, visible_title_width, text_x)) =
                    visible_title_bounds(clip_x, title_width, w)
                {
                    if let Some(title) = fit_clip_title(
                        note_clip.name.as_str(),
                        visible_title_width,
                        note_clip.loop_enabled,
                    ) {
                        frame.with_clip(
                            Rectangle {
                                x: title_x,
                                y: clip_y,
                                width: visible_title_width,
                                height: CLIP_TITLE_HEIGHT,
                            },
                            |title_frame| {
                                title_frame.fill_text(canvas::Text {
                                    content: title,
                                    position: iced::Point::new(text_x, clip_y + 3.0),
                                    color: theme::text(),
                                    size: iced::Pixels(11.0),
                                    ..Default::default()
                                });
                            },
                        );
                    }
                }
            }
        }

        // Loop region tint
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let loop_x1 = self.beat_to_x(self.loop_start_beats);
            let loop_x2 = self.beat_to_x(self.loop_end_beats);
            let fill_x = loop_x1.max(0.0);
            let fill_w = loop_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::accent(), 0.06),
                );
            }
        }

        // Selection region tint. Only drawn on the lane the selection
        // originated on; ruler-drawn selections (track_id = None) still
        // show across every lane.
        let show_selection_on_this_lane = self
            .time_selection_track
            .is_none_or(|tid| tid == self.track_id);
        if show_selection_on_this_lane
            && self.time_selection_active
            && self.selection_end_beats > self.selection_start_beats
        {
            let sel_x1 = self.beat_to_x(self.selection_start_beats);
            let sel_x2 = self.beat_to_x(self.selection_end_beats);
            let fill_x = sel_x1.max(0.0);
            let fill_w = sel_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::accent(), 0.04),
                );
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
                    .with_color(theme::playhead())
                    .with_width(2.0),
            );
        }

        // Bottom separator
        let sep = canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(theme::divider())
                .with_width(1.0),
        );

        // Drop-target indicator: bold accent border + vertical bar at the
        // cursor x + the track name overlaid so the user can verify which
        // lane will receive the drop.
        if drop_hover {
            let target_color = if drop_compatible {
                theme::accent()
            } else {
                theme::danger()
            };
            let outline = canvas::Path::rectangle(
                iced::Point::new(1.0, 1.0),
                iced::Size::new(w - 2.0, h - 2.0),
            );
            frame.stroke(
                &outline,
                canvas::Stroke::default()
                    .with_color(target_color)
                    .with_width(2.0),
            );
            if let Some(local) = cursor.position_in(bounds) {
                let beat = self.snapped_beat(self.x_to_beat(local.x).max(0.0));
                let snapped_x = self.beat_to_x(beat);
                if snapped_x >= 0.0 && snapped_x <= w {
                    let drop_line = canvas::Path::line(
                        iced::Point::new(snapped_x, 0.0),
                        iced::Point::new(snapped_x, h),
                    );
                    frame.stroke(
                        &drop_line,
                        canvas::Stroke::default()
                            .with_color(target_color)
                            .with_width(2.0),
                    );
                }
                if drop_compatible {
                    if let Some(duration_beats) = self.sample_drop_duration_beats {
                        let preview_width = (duration_beats * self.pixels_per_beat() as f64) as f32;
                        let visible_width = preview_width.min((w - snapped_x).max(0.0));
                        if visible_width > 0.0 {
                            frame.fill_rectangle(
                                iced::Point::new(snapped_x, 2.0),
                                iced::Size::new(visible_width, h - 4.0),
                                theme::with_alpha(theme::accent(), 0.18),
                            );
                        }
                    }
                }
                frame.fill_text(canvas::Text {
                    content: if drop_compatible {
                        format!("Beat {beat:.2} · {}", self.track_name)
                    } else {
                        "INVALID · MIDI/instrument lane".into()
                    },
                    position: iced::Point::new(8.0, 8.0),
                    color: target_color,
                    size: 13.0.into(),
                    ..Default::default()
                });
                if drop_compatible {
                    if let Some(detail) = self.sample_drop_detail.as_ref() {
                        frame.fill_text(canvas::Text {
                            content: detail.clone(),
                            position: iced::Point::new(8.0, 28.0),
                            color: theme::text(),
                            size: 11.0.into(),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_clip_title, visible_pixel_columns, visible_title_bounds};

    #[test]
    fn clip_title_is_constrained_to_its_visible_width() {
        assert_eq!(
            fit_clip_title("Kick.wav", 100.0, false),
            Some("Kick.wav".into())
        );
        assert_eq!(
            fit_clip_title("OTH_128_Hub_Full.wav", 80.0, false),
            Some("OTH_128_H..".into())
        );
        assert_eq!(fit_clip_title("Kick.wav", 40.0, false), None);
    }

    #[test]
    fn loop_icon_reserves_title_space() {
        assert_eq!(
            fit_clip_title("OTH_128_Hub_Full.wav", 80.0, true),
            Some("OTH_12..".into())
        );
    }

    #[test]
    fn waveform_iteration_is_limited_to_visible_clip_columns() {
        assert_eq!(
            visible_pixel_columns(-10_000.0, 20_000.0, 1_000.0),
            10_000..11_000
        );
        assert_eq!(visible_pixel_columns(200.0, 400.0, 1_000.0), 0..400);
        assert_eq!(visible_pixel_columns(1_200.0, 400.0, 1_000.0), 0..0);
    }

    #[test]
    fn clip_title_bounds_never_escape_the_arrangement_viewport() {
        assert_eq!(
            visible_title_bounds(-200.0, 400.0, 800.0),
            Some((0.0, 200.0, 4.0))
        );
        assert_eq!(
            visible_title_bounds(100.0, 200.0, 800.0),
            Some((100.0, 200.0, 104.0))
        );
        assert_eq!(visible_title_bounds(900.0, 200.0, 800.0), None);
    }
}
