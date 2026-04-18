use std::collections::HashSet;
use std::time::Instant;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::{PianoRollEditMode, SnapGrid, UiNoteClip};
use crate::theme;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

/// Width of the piano key area on the left.
const KEY_WIDTH: f32 = 52.0;
/// Height of each piano key row.
const KEY_HEIGHT: f32 = 16.0;
/// Height of the ruler strip at the top.
const RULER_HEIGHT: f32 = 20.0;

/// Lowest MIDI note displayed (C2 = 36).
const LOW_NOTE: u8 = 36;
/// Highest MIDI note displayed (C7 = 96).
const HIGH_NOTE: u8 = 96;
/// Total number of note rows.
const NUM_ROWS: u8 = HIGH_NOTE - LOW_NOTE;

/// Resize handle width in pixels.
const RESIZE_HANDLE_PX: f32 = 6.0;

/// Black key width as fraction of KEY_WIDTH.
const BLACK_KEY_RATIO: f32 = 0.60;

/// Scroll speed: pixels per wheel tick.
const SCROLL_SPEED: f32 = 3.0 * KEY_HEIGHT;

/// Piano roll canvas widget.
pub struct PianoRollWidget {
    pub track_id: TrackId,
    pub clip: Option<PianoRollClipData>,
    pub playhead_beats: f64,
    pub total_beats: f64,
    pub track_color: Color,
    pub snap_grid: SnapGrid,
    pub scroll_y: f32,
    pub edit_mode: PianoRollEditMode,
}

/// Owned data for drawing a note clip in the piano roll.
#[derive(Clone)]
pub struct PianoRollClipData {
    pub clip_id: ClipId,
    pub notes: Vec<MidiNote>,
    pub selected_notes: HashSet<usize>,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

impl PianoRollWidget {
    #[allow(clippy::too_many_arguments)]
    pub fn from_clip(
        track_id: TrackId,
        clip: &UiNoteClip,
        playhead_beats: f64,
        total_beats: f64,
        track_color: Color,
        snap_grid: SnapGrid,
        scroll_y: f32,
        edit_mode: PianoRollEditMode,
    ) -> Self {
        Self {
            track_id,
            clip: Some(PianoRollClipData {
                clip_id: clip.id,
                notes: clip.notes.clone(),
                selected_notes: clip.selected_notes.clone(),
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
            }),
            playhead_beats,
            total_beats,
            track_color,
            snap_grid,
            scroll_y,
            edit_mode,
        }
    }

    pub fn empty(track_id: TrackId, playhead_beats: f64, track_color: Color) -> Self {
        Self {
            track_id,
            clip: None,
            playhead_beats,
            total_beats: 16.0,
            track_color,
            snap_grid: SnapGrid::Eighth,
            scroll_y: default_scroll_y(200.0),
            edit_mode: PianoRollEditMode::default(),
        }
    }

    fn beat_to_x(&self, beat: f64, bounds: &Rectangle) -> f32 {
        let grid_width = bounds.width - KEY_WIDTH;
        let total = self.total_beats.max(1.0);
        KEY_WIDTH + (beat / total) as f32 * grid_width
    }

    fn x_to_beat(&self, x: f32, bounds: &Rectangle) -> f64 {
        let grid_width = bounds.width - KEY_WIDTH;
        let total = self.total_beats.max(1.0);
        ((x - KEY_WIDTH) / grid_width) as f64 * total
    }

    fn pitch_to_y(&self, pitch: u8) -> f32 {
        let row = (HIGH_NOTE.saturating_sub(pitch).saturating_sub(1)) as f32;
        row * KEY_HEIGHT + RULER_HEIGHT - self.scroll_y
    }

    fn y_to_pitch(&self, y: f32) -> u8 {
        let adjusted = y - RULER_HEIGHT + self.scroll_y;
        let row = (adjusted / KEY_HEIGHT) as u8;
        HIGH_NOTE.saturating_sub(1).saturating_sub(row)
    }

    /// Hit test: find note at position, return (index, near_right_edge).
    fn hit_test_note(&self, pos: Point, bounds: &Rectangle) -> Option<(usize, bool)> {
        let clip_data = self.clip.as_ref()?;
        let grid_width = bounds.width - KEY_WIDTH;
        let total = self.total_beats.max(1.0);

        for (idx, note) in clip_data.notes.iter().enumerate() {
            if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                continue;
            }

            let x = self.beat_to_x(note.start_beat, bounds);
            let y = self.pitch_to_y(note.pitch);
            let note_w = ((note.duration_beats / total) as f32 * grid_width).max(4.0);
            let note_h = KEY_HEIGHT - 1.0;

            if pos.x >= x && pos.x <= x + note_w && pos.y >= y + 0.5 && pos.y <= y + 0.5 + note_h {
                let near_right = pos.x >= x + note_w - RESIZE_HANDLE_PX;
                return Some((idx, near_right));
            }
        }
        None
    }
}

/// Default scroll_y to center on C3–C5 (musically useful range).
pub fn default_scroll_y(canvas_height: f32) -> f32 {
    let c4_row = (HIGH_NOTE - 60 - 1) as f32;
    let content_height = NUM_ROWS as f32 * KEY_HEIGHT;
    let visible = canvas_height - RULER_HEIGHT;
    (c4_row * KEY_HEIGHT - visible / 2.0).clamp(0.0, (content_height - visible).max(0.0))
}

/// Clamp scroll_y to valid range.
fn clamp_scroll_y(scroll_y: f32, canvas_height: f32) -> f32 {
    let content_height = NUM_ROWS as f32 * KEY_HEIGHT;
    let visible = canvas_height - RULER_HEIGHT;
    scroll_y.clamp(0.0, (content_height - visible).max(0.0))
}

/// Returns a pitch name like "C4", "D#3".
fn pitch_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[pitch as usize % 12],
        (pitch / 12).saturating_sub(1)
    )
}

/// Drag action in progress.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
enum DragAction {
    MoveNote {
        note_index: usize,
        original_pitch: u8,
        original_start_beat: f64,
        /// All selected notes: (index, pitch, start_beat)
        original_notes: Vec<(usize, u8, f64)>,
        start_x: f32,
        start_y: f32,
    },
    ResizeNote {
        note_index: usize,
        original_duration: f64,
        start_x: f32,
    },
    DrawNote {
        clip_id: ClipId,
        pitch: u8,
        start_beat: f64,
        start_x: f32,
    },
}

/// Max time between clicks to count as double-click (ms).
const DOUBLE_CLICK_MS: u128 = 400;
/// Max distance between clicks to count as double-click (px).
const DOUBLE_CLICK_DIST: f32 = 8.0;

/// State for piano roll interaction.
#[derive(Debug, Default)]
pub struct PianoRollState {
    drag: Option<DragAction>,
    last_cursor: Option<Point>,
    shift_held: bool,
    last_click: Option<(Instant, Point)>,
}

// ── Grid row colors ──

/// White key row background: #252525
const WHITE_ROW_BG: Color = Color {
    r: 0.145,
    g: 0.145,
    b: 0.145,
    a: 1.0,
};

/// Black key row background: #1c1c1c
const BLACK_ROW_BG: Color = Color {
    r: 0.110,
    g: 0.110,
    b: 0.110,
    a: 1.0,
};

/// Octave boundary line color: #3a3a3a
const OCTAVE_LINE: Color = Color {
    r: 0.227,
    g: 0.227,
    b: 0.227,
    a: 1.0,
};

/// Normal horizontal grid line: #2a2a2a
const GRID_LINE: Color = Color {
    r: 0.165,
    g: 0.165,
    b: 0.165,
    a: 1.0,
};

/// White key fill for piano: #c8c8c8
const WHITE_KEY_COLOR: Color = Color {
    r: 0.784,
    g: 0.784,
    b: 0.784,
    a: 1.0,
};

/// Black key fill for piano: #1a1a1a
const BLACK_KEY_COLOR: Color = Color {
    r: 0.102,
    g: 0.102,
    b: 0.102,
    a: 1.0,
};

/// Piano key label color for C notes
const KEY_LABEL_COLOR: Color = Color {
    r: 0.35,
    g: 0.35,
    b: 0.35,
    a: 1.0,
};

impl canvas::Program<Message> for PianoRollWidget {
    type State = PianoRollState;

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
        let grid_width = w - KEY_WIDTH;
        let total = self.total_beats.max(1.0);
        let grid_ppb = grid_width / total as f32;

        // Full background
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::BG_DARK);

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
            let row_bg = if is_black { BLACK_ROW_BG } else { WHITE_ROW_BG };

            frame.fill_rectangle(
                iced::Point::new(KEY_WIDTH, y),
                iced::Size::new(grid_width, KEY_HEIGHT),
                row_bg,
            );

            // Horizontal grid line
            let is_c = pitch % 12 == 0;
            let (line_color, line_width) = if is_c {
                (OCTAVE_LINE, 1.0)
            } else {
                (GRID_LINE, 0.5)
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
                frame.fill_rectangle(
                    iced::Point::new(0.0, y),
                    iced::Size::new(KEY_WIDTH, KEY_HEIGHT),
                    WHITE_KEY_COLOR,
                );

                // Key border
                let border = canvas::Path::line(
                    iced::Point::new(0.0, y + KEY_HEIGHT),
                    iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(theme::DIVIDER)
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
                    Color {
                        r: 0.14,
                        g: 0.14,
                        b: 0.14,
                        a: 1.0,
                    },
                );

                // Black key itself
                frame.fill_rectangle(
                    iced::Point::new(0.0, y),
                    iced::Size::new(black_key_width, KEY_HEIGHT),
                    BLACK_KEY_COLOR,
                );

                // Key border
                let border = canvas::Path::line(
                    iced::Point::new(0.0, y + KEY_HEIGHT),
                    iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(theme::DIVIDER)
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

            if pitch % 12 == 0 {
                let label = pitch_name(pitch);
                frame.fill_text(canvas::Text {
                    content: label,
                    position: iced::Point::new(black_key_width + 3.0, y + 2.0),
                    color: KEY_LABEL_COLOR,
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
                .with_color(theme::BORDER)
                .with_width(1.0),
        );

        // ── Vertical beat grid lines (always 16th-note resolution) ──
        let grid_step = 0.25_f64; // always show 16th-note grid
        let num_steps = (total / grid_step).ceil() as usize;

        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            // Snap to half-pixel for crisp 1px rendering (avoids blurry subpixel splits)
            let x = (KEY_WIDTH + (beat / total) as f32 * grid_width).floor() + 0.5;
            if x > w {
                break;
            }
            let beat_int = (beat * 1000.0).round() as i64;
            let is_bar = beat_int % 4000 == 0;
            let is_beat = beat_int % 1000 == 0;

            let (line_color, line_width) = if is_bar {
                (
                    Color {
                        r: 0.376,
                        g: 0.376,
                        b: 0.376,
                        a: 1.0,
                    },
                    1.5,
                ) // #606060
            } else if is_beat {
                (
                    Color {
                        r: 0.251,
                        g: 0.251,
                        b: 0.251,
                        a: 1.0,
                    },
                    1.0,
                ) // #404040
            } else {
                (
                    Color {
                        r: 0.216,
                        g: 0.216,
                        b: 0.216,
                        a: 1.0,
                    },
                    1.0,
                ) // #373737
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
            let selected_color = theme::SOLO_ACTIVE;
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
                let note_w = ((note.duration_beats / total) as f32 * grid_width).max(4.0);
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
                        let gw = ((note.duration_beats / total) as f32 * grid_width).max(4.0);
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
                    .with_color(theme::PLAYHEAD)
                    .with_width(1.5),
            );
        }

        // ── Ruler strip (drawn last so it overlays everything at the top) ──
        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, RULER_HEIGHT),
            theme::BG_SURFACE,
        );

        // Bottom border
        let ruler_border = canvas::Path::line(
            iced::Point::new(0.0, RULER_HEIGHT),
            iced::Point::new(w, RULER_HEIGHT),
        );
        frame.stroke(
            &ruler_border,
            canvas::Stroke::default()
                .with_color(theme::BORDER)
                .with_width(1.0),
        );

        // Ruler tick marks and labels
        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            let x = (KEY_WIDTH + (beat / total) as f32 * grid_width).floor() + 0.5;
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
                        .with_color(theme::TEXT_MUTED)
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{bar_num}"),
                    position: iced::Point::new(x + 3.0, 3.0),
                    color: theme::TEXT_DIM,
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
                        .with_color(theme::TEXT_MUTED)
                        .with_width(0.5),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{}.{}", bar_index + 1, beat_in_bar),
                    position: iced::Point::new(x + 2.0, 5.0),
                    color: theme::TEXT_MUTED,
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
                    .with_color(theme::PLAYHEAD)
                    .with_width(1.5),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            return mouse::Interaction::default();
        }

        // During drag
        if let Some(ref drag) = state.drag {
            return match drag {
                DragAction::MoveNote { .. } => mouse::Interaction::Grabbing,
                DragAction::ResizeNote { .. } | DragAction::DrawNote { .. } => {
                    mouse::Interaction::ResizingHorizontally
                }
            };
        }

        // Draw mode: always crosshair
        if self.edit_mode == PianoRollEditMode::Draw {
            return mouse::Interaction::Crosshair;
        }

        // Select mode: hover over note edge
        if let Some(pos) = cursor.position_in(bounds) {
            if pos.x >= KEY_WIDTH && pos.y > RULER_HEIGHT {
                if let Some((_idx, near_right)) = self.hit_test_note(pos, &bounds) {
                    if near_right {
                        return mouse::Interaction::ResizingHorizontally;
                    }
                    return mouse::Interaction::Grab;
                }
            }
        }

        mouse::Interaction::Crosshair
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            // ── Mouse wheel: vertical scroll ──
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let dy = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => -y * SCROLL_SPEED,
                        mouse::ScrollDelta::Pixels { y, .. } => -y,
                    };
                    let new_scroll = clamp_scroll_y(self.scroll_y + dy, bounds.height);
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::PianoRollScrollY(new_scroll)),
                    );
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Ignore clicks in ruler area
                    if pos.y < RULER_HEIGHT {
                        return (canvas::event::Status::Ignored, None);
                    }

                    // Only handle clicks in the grid area
                    if pos.x < KEY_WIDTH {
                        return (canvas::event::Status::Ignored, None);
                    }

                    state.last_cursor = Some(pos);

                    if let Some(ref clip_data) = self.clip {
                        if self.edit_mode == PianoRollEditMode::Draw {
                            // ── Draw mode ──
                            // Click existing note → delete it
                            if let Some((idx, _)) = self.hit_test_note(pos, &bounds) {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::RemoveNote(
                                        self.track_id,
                                        clip_data.clip_id,
                                        idx,
                                    )),
                                );
                            }

                            // Click empty space → add note + start draw drag
                            let beat = self.x_to_beat(pos.x, &bounds);
                            let pitch = self.y_to_pitch(pos.y);

                            if !(LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                                return (canvas::event::Status::Ignored, None);
                            }

                            let note_duration = self.snap_grid.beat_size();
                            let snapped_beat = self.snap_grid.snap_beat(beat).max(0.0);

                            let max_start = self.total_beats - note_duration;
                            if max_start < 0.0 || snapped_beat > max_start {
                                return (canvas::event::Status::Captured, None);
                            }

                            state.drag = Some(DragAction::DrawNote {
                                clip_id: clip_data.clip_id,
                                pitch,
                                start_beat: snapped_beat,
                                start_x: pos.x,
                            });

                            return (
                                canvas::event::Status::Captured,
                                Some(Message::AddNote {
                                    track_id: self.track_id,
                                    clip_id: clip_data.clip_id,
                                    pitch,
                                    start_beat: snapped_beat,
                                    duration_beats: note_duration,
                                }),
                            );
                        }

                        // ── Select mode (default) ──
                        // Hit test existing notes
                        if let Some((idx, near_right)) = self.hit_test_note(pos, &bounds) {
                            let note = &clip_data.notes[idx];
                            let shift = state.shift_held;

                            if near_right {
                                // Resize always selects just this note
                                state.drag = Some(DragAction::ResizeNote {
                                    note_index: idx,
                                    original_duration: note.duration_beats,
                                    start_x: pos.x,
                                });
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::SelectNote(
                                        self.track_id,
                                        clip_data.clip_id,
                                        Some(idx),
                                        false,
                                    )),
                                );
                            }

                            // Determine the selection set after this click
                            let will_be_selected = if shift {
                                // Toggle: if already selected it stays for drag purposes
                                true
                            } else {
                                true
                            };

                            if will_be_selected {
                                // Build original_notes for multi-note drag
                                // After selection message is processed, the set will include
                                // the clicked note. We need to predict the final selection set.
                                let selected_set: HashSet<usize> = if shift {
                                    let mut s = clip_data.selected_notes.clone();
                                    if !s.remove(&idx) {
                                        s.insert(idx);
                                    }
                                    s
                                } else {
                                    // If clicked note is already in selection, move all selected
                                    if clip_data.selected_notes.contains(&idx) {
                                        clip_data.selected_notes.clone()
                                    } else {
                                        let mut s = HashSet::new();
                                        s.insert(idx);
                                        s
                                    }
                                };

                                let original_notes: Vec<(usize, u8, f64)> = selected_set
                                    .iter()
                                    .filter(|&&i| i < clip_data.notes.len())
                                    .map(|&i| {
                                        (i, clip_data.notes[i].pitch, clip_data.notes[i].start_beat)
                                    })
                                    .collect();

                                state.drag = Some(DragAction::MoveNote {
                                    note_index: idx,
                                    original_pitch: note.pitch,
                                    original_start_beat: note.start_beat,
                                    original_notes,
                                    start_x: pos.x,
                                    start_y: pos.y,
                                });
                            }

                            // If clicked note was already selected and no shift,
                            // don't change selection (allows dragging multi-selection)
                            let msg = if !shift && clip_data.selected_notes.contains(&idx) {
                                None
                            } else {
                                Some(Message::SelectNote(
                                    self.track_id,
                                    clip_data.clip_id,
                                    Some(idx),
                                    shift,
                                ))
                            };

                            return (canvas::event::Status::Captured, msg);
                        }

                        // Select mode empty space:
                        // Double-click → add note, single-click → deselect
                        let is_double = state.last_click.is_some_and(|(t, p)| {
                            t.elapsed().as_millis() < DOUBLE_CLICK_MS
                                && (p.x - pos.x).abs() < DOUBLE_CLICK_DIST
                                && (p.y - pos.y).abs() < DOUBLE_CLICK_DIST
                        });
                        state.last_click = Some((Instant::now(), pos));

                        if is_double {
                            // Double-click: add note
                            let beat = self.x_to_beat(pos.x, &bounds);
                            let pitch = self.y_to_pitch(pos.y);

                            if !(LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                                return (canvas::event::Status::Ignored, None);
                            }

                            let note_duration = self.snap_grid.beat_size();
                            let snapped_beat = self.snap_grid.snap_beat(beat).max(0.0);

                            let max_start = self.total_beats - note_duration;
                            if max_start < 0.0 || snapped_beat > max_start {
                                return (canvas::event::Status::Captured, None);
                            }

                            return (
                                canvas::event::Status::Captured,
                                Some(Message::AddNote {
                                    track_id: self.track_id,
                                    clip_id: clip_data.clip_id,
                                    pitch,
                                    start_beat: snapped_beat,
                                    duration_beats: note_duration,
                                }),
                            );
                        }

                        // Single-click: deselect all
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::SelectNote(
                                self.track_id,
                                clip_data.clip_id,
                                None,
                                false,
                            )),
                        );
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(pos) = cursor.position() {
                    let local = Point::new(pos.x - bounds.x, pos.y - bounds.y);
                    state.last_cursor = Some(local);

                    if let Some(ref drag) = state.drag {
                        if let Some(ref clip_data) = self.clip {
                            let grid_width = bounds.width - KEY_WIDTH;
                            let total = self.total_beats.max(1.0);
                            let beats_per_pixel = total / grid_width as f64;

                            match drag {
                                DragAction::MoveNote {
                                    note_index,
                                    original_pitch,
                                    original_start_beat,
                                    start_x,
                                    start_y,
                                    ..
                                } => {
                                    let dx = local.x - start_x;
                                    let dy = local.y - start_y;
                                    let beat_delta = dx as f64 * beats_per_pixel;
                                    let pitch_delta = -(dy / KEY_HEIGHT).round() as i16;

                                    let anchor_new_beat =
                                        self.snap_grid.snap_beat(original_start_beat + beat_delta);
                                    let snapped_beat_delta = anchor_new_beat - original_start_beat;

                                    // Move anchor note for visual feedback (works for both
                                    // single and multi-note drag; other notes move on release)
                                    let idx = *note_index;
                                    if idx < clip_data.notes.len() {
                                        let max_beat = (self.total_beats
                                            - clip_data.notes[idx].duration_beats)
                                            .max(0.0);
                                        let new_beat = (original_start_beat + snapped_beat_delta)
                                            .clamp(0.0, max_beat);
                                        let new_pitch = (*original_pitch as i16 + pitch_delta)
                                            .clamp(LOW_NOTE as i16, HIGH_NOTE as i16 - 1)
                                            as u8;

                                        let mut note = clip_data.notes[idx];
                                        note.start_beat = new_beat;
                                        note.pitch = new_pitch;
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::EditNote(
                                                self.track_id,
                                                clip_data.clip_id,
                                                idx,
                                                note,
                                            )),
                                        );
                                    }
                                }
                                DragAction::ResizeNote {
                                    note_index,
                                    original_duration,
                                    start_x,
                                } => {
                                    let dx = local.x - start_x;
                                    let beat_delta = dx as f64 * beats_per_pixel;
                                    let min_duration = self.snap_grid.beat_size();
                                    let new_duration = self.snap_grid.snap_beat(
                                        (original_duration + beat_delta).max(min_duration),
                                    );

                                    let idx = *note_index;
                                    if idx < clip_data.notes.len() {
                                        let mut note = clip_data.notes[idx];
                                        note.duration_beats = new_duration;
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::EditNote(
                                                self.track_id,
                                                clip_data.clip_id,
                                                idx,
                                                note,
                                            )),
                                        );
                                    }
                                }
                                DragAction::DrawNote {
                                    clip_id,
                                    pitch,
                                    start_beat,
                                    start_x,
                                } => {
                                    // Extend note duration while dragging
                                    let dx = local.x - start_x;
                                    let beat_delta = dx as f64 * beats_per_pixel;
                                    let min_duration = self.snap_grid.beat_size();
                                    let new_duration = self.snap_grid.snap_beat(
                                        (min_duration + beat_delta.max(0.0)).max(min_duration),
                                    );

                                    // Find the note we just added (last note at this pitch/beat)
                                    if let Some(idx) = clip_data.notes.iter().rposition(|n| {
                                        n.pitch == *pitch
                                            && (n.start_beat - start_beat).abs() < 0.001
                                    }) {
                                        let mut note = clip_data.notes[idx];
                                        note.duration_beats = new_duration;
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::EditNote(
                                                self.track_id,
                                                *clip_id,
                                                idx,
                                                note,
                                            )),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = state.drag.take() {
                    // On release of a multi-note drag, move all non-anchor notes
                    if let DragAction::MoveNote {
                        note_index,
                        original_start_beat,
                        ref original_notes,
                        start_x,
                        start_y,
                        ..
                    } = drag
                    {
                        if original_notes.len() > 1 {
                            if let Some(ref clip_data) = self.clip {
                                if let Some(pos) = state.last_cursor {
                                    let grid_width = bounds.width - KEY_WIDTH;
                                    let total = self.total_beats.max(1.0);
                                    let beats_per_pixel = total / grid_width as f64;

                                    let dx = pos.x - start_x;
                                    let dy = pos.y - start_y;
                                    let beat_delta = dx as f64 * beats_per_pixel;
                                    let pitch_delta = -(dy / KEY_HEIGHT).round() as i16;

                                    let anchor_new_beat =
                                        self.snap_grid.snap_beat(original_start_beat + beat_delta);
                                    let snapped_beat_delta = anchor_new_beat - original_start_beat;

                                    // Compute absolute positions for all selected notes
                                    // (anchor was already moved during drag, non-anchor need moving)
                                    let moves: Vec<(usize, f64, u8)> = original_notes
                                        .iter()
                                        .filter(|(i, _, _)| {
                                            *i != note_index && *i < clip_data.notes.len()
                                        })
                                        .map(|(i, orig_pitch, orig_beat)| {
                                            let new_beat =
                                                (orig_beat + snapped_beat_delta).max(0.0);
                                            let new_pitch = (*orig_pitch as i16 + pitch_delta)
                                                .clamp(LOW_NOTE as i16, HIGH_NOTE as i16 - 1)
                                                as u8;
                                            (*i, new_beat, new_pitch)
                                        })
                                        .collect();

                                    if !moves.is_empty() {
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::MoveNotesAbsolute {
                                                track_id: self.track_id,
                                                clip_id: clip_data.clip_id,
                                                moves,
                                            }),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    return (canvas::event::Status::Captured, None);
                }
            }

            // ── Keyboard: track shift state ──
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
                ..
            }) => {
                state.shift_held = true;
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyReleased {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Shift),
                ..
            }) => {
                state.shift_held = false;
            }

            // ── Keyboard: Delete → remove selected notes ──
            //
            // Backspace is intentionally not bound because iced 0.13 canvas
            // receives keyboard events even when a text input is focused.
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::RemoveSelectedNotes(
                                self.track_id,
                                clip_data.clip_id,
                            )),
                        );
                    }
                }
            }

            // ── Keyboard: Arrow keys → nudge selected notes ──
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: -self.snap_grid.beat_size(),
                                delta_semitones: 0,
                            }),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: self.snap_grid.beat_size(),
                                delta_semitones: 0,
                            }),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: 1,
                            }),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: -1,
                            }),
                        );
                    }
                }
            }

            // ── Keyboard: Ctrl+A → select all notes ──
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character(ref ch),
                modifiers,
                ..
            }) if ch.as_str() == "a" && modifiers.command() => {
                if let Some(ref clip_data) = self.clip {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::SelectAllNotes(self.track_id, clip_data.clip_id)),
                    );
                }
            }

            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

/// Returns true if the given MIDI pitch is a black key.
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}
