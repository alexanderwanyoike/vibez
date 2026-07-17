//! Drawing half of the piano roll canvas.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer};

use crate::theme;

use super::*;

impl PianoRollWidget {
    pub(super) fn draw_impl(
        &self,
        state: &PianoRollState,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Key-lane feedback: the auditioned (pressed) pitch, and the
        // pitch under the cursor while hovering the keys.
        let pressed_pitch = state.audition_pitch;
        let hovered_pitch = cursor.position_in(bounds).and_then(|pos| {
            if pos.x < KEY_WIDTH && pos.y > RULER_HEIGHT && state.audition_pitch.is_none() {
                let pitch = self.y_to_pitch(pos.y);
                (LOW_NOTE..HIGH_NOTE).contains(&pitch).then_some(pitch)
            } else {
                None
            }
        });
        let w = bounds.width;
        let h = bounds.height;
        let grid_width = w - KEY_WIDTH;
        let total = self.total_beats.max(1.0);
        let geometry = self.geometry(&bounds);
        let grid_ppb = geometry.pixels_per_beat();

        // Full background
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::bg_dark());

        // Determine visible row range for culling
        let first_visible = (self.scroll_y / KEY_HEIGHT).floor() as usize;
        let visible_height = h - RULER_HEIGHT;
        let last_visible = ((self.scroll_y + visible_height) / KEY_HEIGHT).ceil() as usize;
        let last_visible = last_visible.min(NUM_ROWS as usize);

        // ── Draw grid row backgrounds ──
        for i in first_visible..last_visible {
            let pitch = HIGH_NOTE - 1 - i as u8;
            let y = i as f32 * KEY_HEIGHT + RULER_HEIGHT - self.scroll_y;

            if y + KEY_HEIGHT < RULER_HEIGHT || y > h {
                continue;
            }

            let is_black = is_black_key(pitch);
            let row_bg = if is_black {
                theme::piano_black_row()
            } else {
                theme::piano_white_row()
            };

            frame.fill_rectangle(
                iced::Point::new(KEY_WIDTH, y),
                iced::Size::new(grid_width, KEY_HEIGHT),
                row_bg,
            );

            // Horizontal grid line
            let is_c = pitch.is_multiple_of(12);
            let (line_color, line_width) = if is_c {
                (theme::piano_octave_line(), 1.0)
            } else {
                (theme::piano_grid(), 0.5)
            };
            let hline = canvas::Path::line(
                iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                iced::Point::new(w, y + KEY_HEIGHT),
            );
            frame.stroke(
                &hline,
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(line_width),
            );
        }

        // ── Draw piano keys ──
        // First pass: draw all white keys
        for i in first_visible..last_visible {
            let pitch = HIGH_NOTE - 1 - i as u8;
            let y = i as f32 * KEY_HEIGHT + RULER_HEIGHT - self.scroll_y;

            if y + KEY_HEIGHT < RULER_HEIGHT || y > h {
                continue;
            }

            if !is_black_key(pitch) {
                let fill = if pressed_pitch == Some(pitch) {
                    self.track_color
                } else if hovered_pitch == Some(pitch) {
                    Color {
                        a: 0.75,
                        ..theme::piano_white_key()
                    }
                } else {
                    theme::piano_white_key()
                };
                frame.fill_rectangle(
                    iced::Point::new(0.0, y),
                    iced::Size::new(KEY_WIDTH, KEY_HEIGHT),
                    fill,
                );

                // Key border
                let border = canvas::Path::line(
                    iced::Point::new(0.0, y + KEY_HEIGHT),
                    iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(theme::divider())
                        .with_width(0.5),
                );
            }
        }

        // Second pass: draw black keys on top (narrower, covering left portion)
        let black_key_width = KEY_WIDTH * BLACK_KEY_RATIO;
        for i in first_visible..last_visible {
            let pitch = HIGH_NOTE - 1 - i as u8;
            let y = i as f32 * KEY_HEIGHT + RULER_HEIGHT - self.scroll_y;

            if y + KEY_HEIGHT < RULER_HEIGHT || y > h {
                continue;
            }

            if is_black_key(pitch) {
                // Dark fill for the background area behind black key
                frame.fill_rectangle(
                    iced::Point::new(0.0, y),
                    iced::Size::new(KEY_WIDTH, KEY_HEIGHT),
                    theme::piano_white_row(),
                );

                // Black key itself
                let fill = if pressed_pitch == Some(pitch) {
                    self.track_color
                } else if hovered_pitch == Some(pitch) {
                    theme::border_light()
                } else {
                    theme::piano_black_key()
                };
                frame.fill_rectangle(
                    iced::Point::new(0.0, y),
                    iced::Size::new(black_key_width, KEY_HEIGHT),
                    fill,
                );

                // Key border
                let border = canvas::Path::line(
                    iced::Point::new(0.0, y + KEY_HEIGHT),
                    iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(theme::divider())
                        .with_width(0.5),
                );
            }
        }

        // C note labels — drawn after keys so they're visible
        for i in first_visible..last_visible {
            let pitch = HIGH_NOTE - 1 - i as u8;
            let y = i as f32 * KEY_HEIGHT + RULER_HEIGHT - self.scroll_y;

            if y + KEY_HEIGHT < RULER_HEIGHT || y > h {
                continue;
            }

            if pitch.is_multiple_of(12) {
                let label = pitch_name(pitch);
                frame.fill_text(canvas::Text {
                    content: label,
                    position: iced::Point::new(black_key_width + 3.0, y + 2.0),
                    color: theme::piano_key_label(),
                    size: iced::Pixels(10.0),
                    ..Default::default()
                });
            }
        }

        // Key area separator (vertical line at right edge of piano)
        let sep = canvas::Path::line(
            iced::Point::new(KEY_WIDTH, RULER_HEIGHT),
            iced::Point::new(KEY_WIDTH, h),
        );
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );

        // ── Vertical musical grid lines ──
        let grid_step = self.grid.effective_grid(grid_ppb).beat_size();
        let num_steps = (total / grid_step).ceil() as usize;

        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            // Snap to half-pixel for crisp 1px rendering (avoids blurry subpixel splits)
            let x = geometry.beat_to_x(beat).floor() + 0.5;
            if x > w {
                break;
            }
            let beat_int = (beat * 1000.0).round() as i64;
            let is_bar = beat_int % 4000 == 0;
            let is_beat = beat_int % 1000 == 0;

            let (line_color, line_width) = if is_bar {
                (theme::grid_bar(), 1.5)
            } else if is_beat {
                (theme::grid_beat(), 1.0)
            } else {
                (theme::grid_sub(), 1.0)
            };

            let vline =
                canvas::Path::line(iced::Point::new(x, RULER_HEIGHT), iced::Point::new(x, h));
            frame.stroke(
                &vline,
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(line_width),
            );
        }

        // ── Draw notes ──
        if let Some(ref clip_data) = self.clip {
            let selected_color = theme::solo_active();
            let looping =
                clip_data.loop_enabled && clip_data.loop_end_beats > clip_data.loop_start_beats;
            let loop_len = if looping {
                clip_data.loop_end_beats - clip_data.loop_start_beats
            } else {
                0.0
            };

            // Draw loop region shading and boundary lines
            if looping {
                let loop_boundary_color = theme::with_alpha(self.track_color, 0.4);
                let loop_shade_color = theme::with_alpha(self.track_color, 0.06);

                // Shade the loop region
                let ls_x = self.beat_to_x(clip_data.loop_start_beats, &bounds);
                let le_x = self.beat_to_x(clip_data.loop_end_beats, &bounds);
                frame.fill_rectangle(
                    iced::Point::new(ls_x, RULER_HEIGHT),
                    iced::Size::new((le_x - ls_x).max(0.0), h - RULER_HEIGHT),
                    loop_shade_color,
                );

                // Loop start line
                let start_line = canvas::Path::line(
                    iced::Point::new(ls_x, RULER_HEIGHT),
                    iced::Point::new(ls_x, h),
                );
                frame.stroke(
                    &start_line,
                    canvas::Stroke::default()
                        .with_color(loop_boundary_color)
                        .with_width(1.5),
                );

                // Loop end line
                let end_line = canvas::Path::line(
                    iced::Point::new(le_x, RULER_HEIGHT),
                    iced::Point::new(le_x, h),
                );
                frame.stroke(
                    &end_line,
                    canvas::Stroke::default()
                        .with_color(loop_boundary_color)
                        .with_width(1.5),
                );

                // Draw repeat boundary lines in the looped region
                let mut boundary_beat = clip_data.loop_end_beats + loop_len;
                while boundary_beat < total {
                    let bx = self.beat_to_x(boundary_beat, &bounds);
                    let bline = canvas::Path::line(
                        iced::Point::new(bx, RULER_HEIGHT),
                        iced::Point::new(bx, h),
                    );
                    frame.stroke(
                        &bline,
                        canvas::Stroke::default()
                            .with_color(theme::with_alpha(self.track_color, 0.2))
                            .with_width(0.5),
                    );
                    boundary_beat += loop_len;
                }
            }

            for (idx, note) in clip_data.notes.iter().enumerate() {
                if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                    continue;
                }

                let x = self.beat_to_x(note.start_beat, &bounds);
                let y = self.pitch_to_y(note.pitch);
                let note_w = geometry.width_for_beats(note.duration_beats).max(4.0);
                let note_h = KEY_HEIGHT - 1.0;

                // Skip off-screen notes
                if y + note_h < RULER_HEIGHT || y > h {
                    continue;
                }

                let is_selected = clip_data.selected_notes.contains(&idx);

                // Velocity-based alpha: 0.3 + (velocity / 127) * 0.7
                let vel_alpha = 0.3 + (note.velocity as f32 / 127.0) * 0.7;
                let color = if is_selected {
                    selected_color
                } else {
                    theme::with_alpha(self.track_color, vel_alpha)
                };

                frame.fill_rectangle(
                    iced::Point::new(x, y + 0.5),
                    iced::Size::new(note_w, note_h),
                    color,
                );

                // Velocity indicator line (white, width proportional to velocity)
                let vel_fraction = note.velocity as f32 / 127.0;
                let vel_line_width = note_w * vel_fraction;
                let vel_y = y + 0.5 + note_h - 2.0;
                let vel_line = canvas::Path::line(
                    iced::Point::new(x, vel_y),
                    iced::Point::new(x + vel_line_width, vel_y),
                );
                frame.stroke(
                    &vel_line,
                    canvas::Stroke::default()
                        .with_color(Color::WHITE)
                        .with_width(1.5),
                );

                // Resize handle: highlight on right edge
                let handle_color = if is_selected {
                    Color::WHITE
                } else {
                    theme::with_alpha(self.track_color, vel_alpha + 0.15)
                };
                let handle_w = RESIZE_HANDLE_PX.min(note_w * 0.5).max(2.0);
                frame.fill_rectangle(
                    iced::Point::new(x + note_w - handle_w, y + 0.5),
                    iced::Size::new(handle_w, note_h),
                    handle_color,
                );

                // Note border
                let note_border = canvas::Path::rectangle(
                    iced::Point::new(x, y + 0.5),
                    iced::Size::new(note_w, note_h),
                );
                frame.stroke(
                    &note_border,
                    canvas::Stroke::default()
                        .with_color(theme::darken(self.track_color, 0.6))
                        .with_width(0.5),
                );

                // Note label (when wide enough)
                if note_w > 30.0 {
                    frame.fill_text(canvas::Text {
                        content: pitch_name(note.pitch),
                        position: iced::Point::new(x + 2.0, y + 3.0),
                        color: Color::WHITE,
                        size: iced::Pixels(8.0),
                        ..Default::default()
                    });
                }
            }

            // ── Draw ghost notes in looped region ──
            if looping && !clip_data.notes.is_empty() {
                let ghost_alpha = 0.25;
                let ghost_color = theme::with_alpha(self.track_color, ghost_alpha);
                let ghost_border =
                    theme::with_alpha(theme::darken(self.track_color, 0.6), ghost_alpha);

                let mut offset_beat = loop_len;
                while clip_data.loop_end_beats + offset_beat - loop_len < total {
                    for note in &clip_data.notes {
                        if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                            continue;
                        }
                        // Only draw notes within the loop region
                        if note.start_beat < clip_data.loop_start_beats
                            || note.start_beat >= clip_data.loop_end_beats
                        {
                            continue;
                        }

                        let ghost_beat = note.start_beat + offset_beat;
                        if ghost_beat >= total {
                            continue;
                        }

                        let gx = self.beat_to_x(ghost_beat, &bounds);
                        let gy = self.pitch_to_y(note.pitch);
                        let gw = geometry.width_for_beats(note.duration_beats).max(4.0);
                        let gh = KEY_HEIGHT - 1.0;

                        if gy + gh < RULER_HEIGHT || gy > h {
                            continue;
                        }

                        frame.fill_rectangle(
                            iced::Point::new(gx, gy + 0.5),
                            iced::Size::new(gw, gh),
                            ghost_color,
                        );
                        let gb = canvas::Path::rectangle(
                            iced::Point::new(gx, gy + 0.5),
                            iced::Size::new(gw, gh),
                        );
                        frame.stroke(
                            &gb,
                            canvas::Stroke::default()
                                .with_color(ghost_border)
                                .with_width(0.5),
                        );
                    }
                    offset_beat += loop_len;
                }
            }
        }

        // ── Playhead ──
        let playhead_x = self.beat_to_x(self.playhead_beats, &bounds);
        if playhead_x >= KEY_WIDTH {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, RULER_HEIGHT),
                iced::Point::new(playhead_x, h),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(1.5),
            );
        }

        // ── Ruler strip (drawn last so it overlays everything at the top) ──
        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, RULER_HEIGHT),
            theme::bg_surface(),
        );

        // Bottom border
        let ruler_border = canvas::Path::line(
            iced::Point::new(0.0, RULER_HEIGHT),
            iced::Point::new(w, RULER_HEIGHT),
        );
        frame.stroke(
            &ruler_border,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );

        // Ruler tick marks and labels
        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            let x = geometry.beat_to_x(beat).floor() + 0.5;
            if x > w {
                break;
            }
            let beat_int = (beat * 1000.0).round() as i64;
            let is_bar = beat_int % 4000 == 0;
            let is_beat = beat_int % 1000 == 0;

            if is_bar {
                let bar_num = (beat / 4.0) as usize + 1;
                // Tick mark
                let tick = canvas::Path::line(
                    iced::Point::new(x, RULER_HEIGHT - 6.0),
                    iced::Point::new(x, RULER_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{bar_num}"),
                    position: iced::Point::new(x + 3.0, 3.0),
                    color: theme::text_dim(),
                    size: iced::Pixels(10.0),
                    ..Default::default()
                });
            } else if is_beat && grid_ppb > 40.0 {
                let bar_index = (beat / 4.0).floor() as usize;
                let beat_in_bar = ((beat % 4.0) as usize) + 1;
                // Smaller tick
                let tick = canvas::Path::line(
                    iced::Point::new(x, RULER_HEIGHT - 3.0),
                    iced::Point::new(x, RULER_HEIGHT),
                );
                frame.stroke(
                    &tick,
                    canvas::Stroke::default()
                        .with_color(theme::text_muted())
                        .with_width(0.5),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{}.{}", bar_index + 1, beat_in_bar),
                    position: iced::Point::new(x + 2.0, 5.0),
                    color: theme::text_muted(),
                    size: iced::Pixels(8.0),
                    ..Default::default()
                });
            }
        }

        // Ruler playhead marker
        if playhead_x >= KEY_WIDTH {
            let marker = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, RULER_HEIGHT),
            );
            frame.stroke(
                &marker,
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}
