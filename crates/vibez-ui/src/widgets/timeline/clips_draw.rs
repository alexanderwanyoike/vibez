//! Drawing half of the clip lane canvas.

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer};

use crate::theme;

use super::clip_colors::{clip_colors, ClipColors};
use super::clip_frame::{clip_box, draw_clip_back, draw_clip_front, LoopPasses};
use super::fade_drag::{fade_curve_handle, fade_handle_xs};
use super::lane_grid::{lane_grid, GridLine};
use super::note_marks::note_marks;
use super::*;

/// How far to shift the lane so whole-pixel coordinates inside it land on
/// whole screen pixels.
fn whole_pixel_offset(origin: iced::Point) -> iced::Vector {
    iced::Vector::new(-origin.x.fract(), -origin.y.fract())
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

fn waveform_peak_for_pixel(
    peaks: &[(f32, f32)],
    pixel: usize,
    rendered_pixels: usize,
    frames_per_pixel: f64,
    peak_span_frames: Option<usize>,
) -> Option<(f32, f32)> {
    if let Some(peak_span_frames) = peak_span_frames.filter(|span| *span > 0) {
        let peak_span_frames = peak_span_frames as f64;
        let first = (pixel as f64 * frames_per_pixel / peak_span_frames).floor() as usize;
        let end = (((pixel + 1) as f64 * frames_per_pixel / peak_span_frames).ceil() as usize)
            .max(first + 1)
            .min(peaks.len());
        let visible = peaks.get(first..end)?;
        return visible
            .iter()
            .copied()
            .reduce(|combined, peak| (combined.0.min(peak.0), combined.1.max(peak.1)));
    }

    peaks
        .get(pixel.saturating_mul(peaks.len()) / rendered_pixels.max(1))
        .copied()
}

impl TrackClipCanvas {
    fn clip_colors(&self, selected: bool, recording_preview: bool) -> ClipColors {
        let track = if recording_preview {
            theme::danger()
        } else {
            self.track_color
        };
        clip_colors(track, selected)
    }

    pub(super) fn draw_impl(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // Layout can place the lane between pixel rows, which would smear
        // every clip edge and one pixel line across two rows.
        frame.translate(whole_pixel_offset(bounds.position()));
        let w = bounds.width;
        let h = bounds.height;
        let geometry = self.geometry();
        let ppb = geometry.pixels_per_beat();

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
        // One pixel larger to cover what the whole-pixel shift uncovers.
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w + 1.0, h + 1.0),
            bg_color,
        );

        // Grid lines use the same effective division as interaction.
        if self.bpm > 0.0 {
            let step = self.grid.effective_grid(ppb).beat_size();
            let grid = lane_grid(geometry, w, step);
            let style = theme::lane_style();
            let ink = theme::text();
            // Mixed here rather than painted translucent: the renderer
            // blends alpha in linear light, which would make these faint
            // tints several times stronger than their tokens.
            let band = theme::blend(bg_color, ink, style.band);
            for &(start, end) in &grid.bands {
                frame.fill_rectangle(
                    iced::Point::new(start, 0.0),
                    iced::Size::new(end - start, h),
                    band,
                );
            }
            for (x, line) in grid.lines {
                let strength = match line {
                    GridLine::Label => style.lines[0],
                    GridLine::Bar => style.lines[1],
                    GridLine::Fine => style.lines[2],
                };
                let under = if grid
                    .bands
                    .iter()
                    .any(|&(start, end)| (start..end).contains(&x))
                {
                    band
                } else {
                    bg_color
                };
                frame.fill_rectangle(
                    iced::Point::new(x, 0.0),
                    iced::Size::new(1.0, h),
                    theme::blend(under, ink, strength),
                );
            }
        }

        // Draw audio clips
        if self.bpm > 0.0 {
            let spb = self.sample_rate as f64 * 60.0 / self.bpm;
            for (clip, is_recording_preview) in self
                .clips
                .iter()
                .map(|clip| (clip, false))
                .chain(self.audio_recording_preview.iter().map(|clip| (clip, true)))
            {
                let is_selected =
                    !is_recording_preview && self.selected_clips.contains(&clip.clip_id);
                let colors = self.clip_colors(is_selected, is_recording_preview);
                let clip_start_beat = clip.position as f64 / spb;
                let clip_dur_beats = clip.duration as f64 / spb;

                let clip_x = self.beat_to_x(clip_start_beat);
                let clip_w = geometry.width_for_beats(clip_dur_beats);

                // Skip clips entirely outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = CLIP_Y;
                let clip_h = h - 2.0 * CLIP_Y;
                let body = clip_box(clip_x, clip_y, clip_w, clip_h);

                let looped = clip.loop_enabled && clip.loop_end > clip.loop_start;
                let loop_passes = looped.then(|| {
                    let pass_px =
                        geometry.width_for_beats((clip.loop_end - clip.loop_start) as f64 / spb);
                    LoopPasses {
                        first_wrap_px: pass_px,
                        pass_px,
                    }
                });
                draw_clip_back(&mut frame, body, &colors, loop_passes, w);

                if !clip.peaks.is_empty() && clip_w > 4.0 {
                    let body_top = clip_y + CLIP_TITLE_HEIGHT;
                    let body_h = clip_h - CLIP_TITLE_HEIGHT;
                    let center_y = body_top + body_h / 2.0;
                    let half_h = body_h / 2.0 - 2.0;
                    let pixels = clip_w as usize;
                    let frames_per_pixel = spb / f64::from(ppb);
                    for px in visible_pixel_columns(clip_x, body.width, w) {
                        let screen_x = clip_x + px as f32;
                        let Some((min_val, max_val)) = waveform_peak_for_pixel(
                            &clip.peaks,
                            px,
                            pixels,
                            frames_per_pixel,
                            clip.peak_span_frames,
                        ) else {
                            break;
                        };
                        let y_top = center_y - (max_val * half_h);
                        let y_bottom = center_y - (min_val * half_h);
                        let height = (y_bottom - y_top).max(1.0);
                        frame.fill_rectangle(
                            iced::Point::new(screen_x, y_top),
                            iced::Size::new(1.0, height),
                            colors.ink,
                        );
                    }
                }

                if !is_recording_preview
                    && (is_selected || clip.fade_in_frames > 0 || clip.fade_out_frames > 0)
                {
                    let (fade_in_x, fade_out_x) = fade_handle_xs(clip_x, clip_w, clip);
                    let top = clip_y + CLIP_TITLE_HEIGHT + 2.0;
                    let bottom = clip_y + clip_h - 2.0;
                    let envelope = canvas::Path::new(|builder| {
                        builder.move_to(iced::Point::new(clip_x, bottom));
                        for step in 1..=12 {
                            let progress = step as f32 / 12.0;
                            let x = clip_x + (fade_in_x - clip_x) * progress;
                            let y = bottom
                                - (bottom - top)
                                    * clip
                                        .fade_in_curve
                                        .gain_for(progress, clip.crossfade_in_from.is_some());
                            builder.line_to(iced::Point::new(x, y));
                        }
                        builder.line_to(iced::Point::new(fade_out_x, top));
                        for step in 1..=12 {
                            let progress = step as f32 / 12.0;
                            let x = fade_out_x + (clip_x + clip_w - fade_out_x) * progress;
                            let gain = clip
                                .fade_out_curve
                                .fade_out_gain(progress, clip.crossfade_out_to.is_some());
                            let y = bottom - (bottom - top) * gain;
                            builder.line_to(iced::Point::new(x, y));
                        }
                    });
                    frame.stroke(
                        &envelope,
                        canvas::Stroke::default()
                            .with_color(theme::with_alpha(theme::text(), 0.72))
                            .with_width(1.25),
                    );
                    if is_selected {
                        for x in [fade_in_x, fade_out_x] {
                            frame.fill_rectangle(
                                iced::Point::new(x - 2.5, FADE_HANDLE_Y - 2.5),
                                iced::Size::new(5.0, 5.0),
                                theme::accent(),
                            );
                        }
                        for edge in [
                            crate::state::AudioClipFadeEdge::In,
                            crate::state::AudioClipFadeEdge::Out,
                        ] {
                            if let Some(handle) = fade_curve_handle(clip_x, clip_w, h, clip, edge) {
                                frame.fill(&canvas::Path::circle(handle, 3.25), theme::accent());
                            }
                        }
                    }
                }
                draw_clip_front(&mut frame, body, &colors, &clip.name, looped, w);

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
                        let a_x = x0.clamp(clip_x, body.x + body.width);
                        let b_x = x1.clamp(clip_x, body.x + body.width);
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

        // Keep one visible marker for every linked relationship. Selecting
        // either Clip reveals the shared curve handle at its actual height.
        for (outgoing_id, incoming_id, handle) in self.crossfade_curve_handles(h) {
            let marker = canvas::Path::new(|builder| {
                builder.move_to(iced::Point::new(handle.x - 3.0, CLIP_Y + 4.0));
                builder.line_to(iced::Point::new(handle.x + 3.0, CLIP_Y + 10.0));
                builder.move_to(iced::Point::new(handle.x + 3.0, CLIP_Y + 4.0));
                builder.line_to(iced::Point::new(handle.x - 3.0, CLIP_Y + 10.0));
            });
            frame.stroke(
                &marker,
                canvas::Stroke::default()
                    .with_color(theme::accent())
                    .with_width(1.4),
            );
            if self.selected_clips.contains(&outgoing_id)
                || self.selected_clips.contains(&incoming_id)
            {
                frame.fill(&canvas::Path::circle(handle, 4.0), theme::accent());
                frame.stroke(
                    &canvas::Path::circle(handle, 4.0),
                    canvas::Stroke::default()
                        .with_color(theme::text())
                        .with_width(1.0),
                );
            }
        }

        // Draw note clips (for instrument tracks)
        if self.is_instrument && self.bpm > 0.0 {
            for (note_clip, is_recording_preview) in self
                .note_clips
                .iter()
                .map(|clip| (clip, false))
                .chain(self.recording_preview.iter().map(|clip| (clip, true)))
            {
                let is_selected =
                    !is_recording_preview && self.selected_clips.contains(&note_clip.clip_id);
                let colors = self.clip_colors(is_selected, is_recording_preview);
                let clip_x = self.beat_to_x(note_clip.position_beats);
                let clip_w = geometry.width_for_beats(note_clip.duration_beats);

                // Skip clips outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = CLIP_Y;
                let clip_h = h - 2.0 * CLIP_Y;
                let body = clip_box(clip_x, clip_y, clip_w, clip_h);

                let looping =
                    note_clip.loop_enabled && note_clip.loop_end_beats > note_clip.loop_start_beats;
                let loop_passes =
                    note_clip
                        .timeline()
                        .first_wrap()
                        .filter(|_| looping)
                        .map(|first_wrap| LoopPasses {
                            first_wrap_px: geometry.width_for_beats(first_wrap),
                            pass_px: geometry.width_for_beats(
                                note_clip.loop_end_beats - note_clip.loop_start_beats,
                            ),
                        });
                draw_clip_back(&mut frame, body, &colors, loop_passes, w);

                // Draw note blocks inside the clip (below title bar)
                let left = clip_x.max(0.0);
                let area = Rectangle {
                    x: left,
                    y: clip_y + CLIP_TITLE_HEIGHT,
                    width: ((body.x + body.width).min(w) - left).max(0.0),
                    height: clip_h - CLIP_TITLE_HEIGHT,
                };
                for mark in note_marks(&note_clip.notes, clip_x, area, geometry.pixels_per_beat()) {
                    frame.fill_rectangle(mark.position(), mark.size(), colors.ink);
                }

                draw_clip_front(&mut frame, body, &colors, &note_clip.name, looping, w);
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

        // Rubber-band slice. Every lane draws its own portion of the same
        // column-space box, so the pieces line up into one rectangle.
        if let Some(marquee) = self.marquee {
            if let Some((top, bottom)) =
                super::marquee::slice_for_lane(marquee.top_y, marquee.bottom_y, self.row_top(), h)
            {
                let x0 = self.beat_to_x(marquee.start_beats).max(0.0);
                let x1 = self.beat_to_x(marquee.end_beats).min(w);
                if x1 > x0 {
                    let origin = iced::Point::new(x0, top);
                    let size = iced::Size::new(x1 - x0, bottom - top);
                    frame.fill_rectangle(origin, size, theme::with_alpha(theme::accent(), 0.10));
                    frame.stroke(
                        &canvas::Path::rectangle(origin, size),
                        canvas::Stroke::default()
                            .with_color(theme::accent())
                            .with_width(1.0),
                    );
                }
            }
        }

        // A playing Section has an engine-owned playback marker in addition
        // to its runtime edit cursor. Draw it first and in the accent colour
        // so overlapping markers remain recognisable.
        if let Some(playback_beat) = self.playback_playhead_beats {
            let playback_x = self.beat_to_x(playback_beat);
            if playback_x >= 0.0 && playback_x <= w {
                let playback_line = canvas::Path::line(
                    iced::Point::new(playback_x, 0.0),
                    iced::Point::new(playback_x, h),
                );
                frame.stroke(
                    &playback_line,
                    canvas::Stroke::default()
                        .with_color(theme::with_alpha(theme::accent(), 0.8))
                        .with_width(3.0),
                );
            }
        }

        // Edit cursor overlay (Arrange uses its shared transport/edit cursor).
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
                        let preview_width = geometry.width_for_beats(duration_beats);
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
    use super::{visible_pixel_columns, waveform_peak_for_pixel, whole_pixel_offset};

    #[test]
    fn a_lane_laid_out_between_pixel_rows_paints_on_whole_pixels() {
        let origin = iced::Point::new(640.25, 173.5);

        let painted = origin + whole_pixel_offset(origin);

        assert_eq!((painted.x.fract(), painted.y.fract()), (0.0, 0.0));
        assert!(origin.distance(painted) < 1.0);
    }

    #[test]
    fn a_live_waveform_pixel_keeps_its_source_range_as_the_take_grows() {
        let first: Vec<_> = (0..120).map(|index| (index as f32, index as f32)).collect();
        let grown: Vec<_> = (0..132).map(|index| (index as f32, index as f32)).collect();

        let before = waveform_peak_for_pixel(&first, 10, 12, 600.0, Some(64));
        let after = waveform_peak_for_pixel(&grown, 10, 14, 600.0, Some(64));

        assert_eq!(before, after);
        assert_eq!(before, Some((93.0, 103.0)));
    }

    #[test]
    fn compacting_live_peaks_keeps_the_same_pixel_extrema() {
        let original: Vec<_> = (0..240)
            .map(|index| (-(index as f32), index as f32))
            .collect();
        let compacted: Vec<_> = original
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| (pair[0].0.min(pair[1].0), pair[0].1.max(pair[1].1)))
            .collect();

        let before = waveform_peak_for_pixel(&original, 10, 24, 600.0, Some(64));
        let after = waveform_peak_for_pixel(&compacted, 10, 24, 600.0, Some(128));

        assert_eq!(before, after);
    }

    #[test]
    fn fitted_clip_peaks_keep_the_existing_width_based_sampling() {
        let peaks = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];

        assert_eq!(
            waveform_peak_for_pixel(&peaks, 2, 8, 600.0, None),
            Some((1.0, 1.0))
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
}
