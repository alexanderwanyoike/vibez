use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::{SnapGrid, UiNoteClip};
use crate::theme;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

/// Width of the piano key area on the left.
const KEY_WIDTH: f32 = 40.0;
/// Height of each piano key row.
const KEY_HEIGHT: f32 = 14.0;

/// Lowest MIDI note displayed (C2 = 36).
const LOW_NOTE: u8 = 36;
/// Highest MIDI note displayed (C7 = 96).
const HIGH_NOTE: u8 = 96;
/// Total number of note rows.
const NUM_ROWS: u8 = HIGH_NOTE - LOW_NOTE;

/// Resize handle width in pixels.
const RESIZE_HANDLE_PX: f32 = 6.0;

/// Piano roll canvas widget.
pub struct PianoRollWidget {
    pub track_id: TrackId,
    pub clip: Option<PianoRollClipData>,
    pub playhead_beats: f64,
    pub total_beats: f64,
    pub track_color: Color,
    pub snap_grid: SnapGrid,
}

/// Owned data for drawing a note clip in the piano roll.
#[derive(Clone)]
pub struct PianoRollClipData {
    pub clip_id: ClipId,
    pub notes: Vec<MidiNote>,
    pub selected_note: Option<usize>,
}

impl PianoRollWidget {
    pub fn from_clip(
        track_id: TrackId,
        clip: &UiNoteClip,
        playhead_beats: f64,
        total_beats: f64,
        track_color: Color,
        snap_grid: SnapGrid,
    ) -> Self {
        Self {
            track_id,
            clip: Some(PianoRollClipData {
                clip_id: clip.id,
                notes: clip.notes.clone(),
                selected_note: clip.selected_note,
            }),
            playhead_beats,
            total_beats,
            track_color,
            snap_grid,
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

    fn pitch_to_y(pitch: u8) -> f32 {
        let row = (HIGH_NOTE.saturating_sub(pitch).saturating_sub(1)) as f32;
        row * KEY_HEIGHT
    }

    fn y_to_pitch(y: f32) -> u8 {
        let row = (y / KEY_HEIGHT) as u8;
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
            let y = Self::pitch_to_y(note.pitch);
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

/// Drag action in progress.
#[derive(Debug, Clone)]
enum DragAction {
    MoveNote {
        note_index: usize,
        original_pitch: u8,
        original_start_beat: f64,
        start_x: f32,
        start_y: f32,
    },
    ResizeNote {
        note_index: usize,
        original_duration: f64,
        start_x: f32,
    },
}

/// State for piano roll interaction.
#[derive(Debug, Default)]
pub struct PianoRollState {
    drag: Option<DragAction>,
    last_cursor: Option<Point>,
}

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

        // Background
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::BG_DARK);

        let grid_width = w - KEY_WIDTH;
        let total_height = NUM_ROWS as f32 * KEY_HEIGHT;

        // Draw piano keys
        for i in 0..NUM_ROWS {
            let pitch = HIGH_NOTE - 1 - i;
            let y = i as f32 * KEY_HEIGHT;
            let is_black = is_black_key(pitch);

            let key_color = if is_black {
                theme::BG_DARK
            } else {
                theme::BG_SURFACE
            };

            frame.fill_rectangle(
                iced::Point::new(0.0, y),
                iced::Size::new(KEY_WIDTH, KEY_HEIGHT),
                key_color,
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

            // Note name label on C notes
            if pitch % 12 == 0 {
                let octave = pitch / 12;
                let label = format!("C{}", octave.saturating_sub(1));
                frame.fill_text(canvas::Text {
                    content: label,
                    position: iced::Point::new(3.0, y + 2.0),
                    color: theme::TEXT_DIM,
                    size: iced::Pixels(9.0),
                    ..Default::default()
                });
            }

            // Grid row background (alternating for black keys)
            let row_bg = if is_black {
                Color {
                    r: 0.08,
                    g: 0.08,
                    b: 0.08,
                    a: 1.0,
                }
            } else {
                Color {
                    r: 0.11,
                    g: 0.11,
                    b: 0.11,
                    a: 1.0,
                }
            };
            frame.fill_rectangle(
                iced::Point::new(KEY_WIDTH, y),
                iced::Size::new(grid_width, KEY_HEIGHT),
                row_bg,
            );

            // Horizontal grid line
            let hline = canvas::Path::line(
                iced::Point::new(KEY_WIDTH, y + KEY_HEIGHT),
                iced::Point::new(w, y + KEY_HEIGHT),
            );
            frame.stroke(
                &hline,
                canvas::Stroke::default()
                    .with_color(theme::DIVIDER)
                    .with_width(0.5),
            );
        }

        // Vertical beat grid lines (with snap grid subdivisions)
        let total = self.total_beats.max(1.0);
        let grid_step = self.snap_grid.beat_size();
        let num_steps = (total / grid_step).ceil() as usize;
        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            let x = KEY_WIDTH + (beat / total) as f32 * grid_width;
            if x > w {
                break;
            }
            let beat_int = (beat * 1000.0).round() as i64;
            let is_bar = beat_int % 4000 == 0;
            let is_beat = beat_int % 1000 == 0;
            let (line_color, line_width) = if is_bar {
                (theme::BORDER, 1.0)
            } else if is_beat {
                (theme::DIVIDER, 0.7)
            } else {
                (
                    Color {
                        r: 0.15,
                        g: 0.15,
                        b: 0.15,
                        a: 1.0,
                    },
                    0.3,
                )
            };

            let vline = canvas::Path::line(
                iced::Point::new(x, 0.0),
                iced::Point::new(x, total_height.min(h)),
            );
            frame.stroke(
                &vline,
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(line_width),
            );

            // Bar number labels
            if is_bar {
                let bar_num = (beat / 4.0) as usize + 1;
                frame.fill_text(canvas::Text {
                    content: format!("{bar_num}"),
                    position: iced::Point::new(x + 2.0, 1.0),
                    color: theme::TEXT_MUTED,
                    size: iced::Pixels(9.0),
                    ..Default::default()
                });
            }
        }

        // Draw notes using track color with velocity-based opacity
        if let Some(ref clip_data) = self.clip {
            let selected_color = theme::SOLO_ACTIVE;

            for (idx, note) in clip_data.notes.iter().enumerate() {
                if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                    continue;
                }

                let x = self.beat_to_x(note.start_beat, &bounds);
                let y = Self::pitch_to_y(note.pitch);
                let note_w = ((note.duration_beats / total) as f32 * grid_width).max(4.0);
                let note_h = KEY_HEIGHT - 1.0;

                let is_selected = clip_data.selected_note == Some(idx);

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

                // Resize handle: 3px highlight on right edge
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
            }
        }

        // Playhead
        let playhead_x = self.beat_to_x(self.playhead_beats, &bounds);
        if playhead_x >= KEY_WIDTH {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, total_height.min(h)),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(1.5),
            );
        }

        // Key area separator
        let sep = canvas::Path::line(
            iced::Point::new(KEY_WIDTH, 0.0),
            iced::Point::new(KEY_WIDTH, h),
        );
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(theme::BORDER)
                .with_width(1.0),
        );

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
                DragAction::ResizeNote { .. } => mouse::Interaction::ResizingHorizontally,
            };
        }

        // Hover over note edge
        if let Some(pos) = cursor.position_in(bounds) {
            if pos.x >= KEY_WIDTH {
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
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Only handle clicks in the grid area
                    if pos.x < KEY_WIDTH {
                        return (canvas::event::Status::Ignored, None);
                    }

                    state.last_cursor = Some(pos);

                    if let Some(ref clip_data) = self.clip {
                        // Hit test existing notes
                        if let Some((idx, near_right)) = self.hit_test_note(pos, &bounds) {
                            let note = &clip_data.notes[idx];
                            if near_right {
                                // Start resize drag
                                state.drag = Some(DragAction::ResizeNote {
                                    note_index: idx,
                                    original_duration: note.duration_beats,
                                    start_x: pos.x,
                                });
                            } else {
                                // Start move drag and select
                                state.drag = Some(DragAction::MoveNote {
                                    note_index: idx,
                                    original_pitch: note.pitch,
                                    original_start_beat: note.start_beat,
                                    start_x: pos.x,
                                    start_y: pos.y,
                                });
                            }
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::SelectNote(
                                    self.track_id,
                                    clip_data.clip_id,
                                    Some(idx),
                                )),
                            );
                        }

                        // Click on empty space: add a note
                        let beat = self.x_to_beat(pos.x, &bounds);
                        let pitch = Self::y_to_pitch(pos.y);

                        if !(LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                            return (canvas::event::Status::Ignored, None);
                        }

                        let snapped_beat = self.snap_grid.snap_beat(beat);
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::AddNote {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                pitch,
                                start_beat: snapped_beat.max(0.0),
                                duration_beats: self.snap_grid.beat_size(),
                            }),
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
                                } => {
                                    let dx = local.x - start_x;
                                    let dy = local.y - start_y;
                                    let beat_delta = dx as f64 * beats_per_pixel;
                                    let pitch_delta = -(dy / KEY_HEIGHT).round() as i16;

                                    let new_beat = self
                                        .snap_grid
                                        .snap_beat(original_start_beat + beat_delta)
                                        .max(0.0);
                                    let new_pitch = (*original_pitch as i16 + pitch_delta)
                                        .clamp(LOW_NOTE as i16, HIGH_NOTE as i16 - 1)
                                        as u8;

                                    let idx = *note_index;
                                    if idx < clip_data.notes.len() {
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
                            }
                        }
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.is_some() {
                    state.drag = None;
                    return (canvas::event::Status::Captured, None);
                }
            }

            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace),
                ..
            })
            | canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if let Some(selected_idx) = clip_data.selected_note {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::RemoveNote(
                                self.track_id,
                                clip_data.clip_id,
                                selected_idx,
                            )),
                        );
                    }
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
