use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::state::UiNoteClip;
use crate::theme;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

/// Width of the piano key area on the left.
const KEY_WIDTH: f32 = 40.0;
/// Height of each piano key row.
const KEY_HEIGHT: f32 = 14.0;
/// Pixels per beat on the horizontal axis (used for absolute-size rendering).
#[allow(dead_code)]
const PIXELS_PER_BEAT: f32 = 80.0;

/// Lowest MIDI note displayed (C2 = 36).
const LOW_NOTE: u8 = 36;
/// Highest MIDI note displayed (C7 = 96).
const HIGH_NOTE: u8 = 96;
/// Total number of note rows.
const NUM_ROWS: u8 = HIGH_NOTE - LOW_NOTE;

/// Default duration for newly placed notes (in beats).
const DEFAULT_NOTE_DURATION: f64 = 0.5;

/// Color for note rectangles.
const NOTE_COLOR: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 0.85,
};

/// Color for selected note.
const NOTE_SELECTED_COLOR: Color = Color {
    r: 1.0,
    g: 0.843,
    b: 0.0,
    a: 0.9,
};

/// Piano roll canvas widget.
pub struct PianoRollWidget {
    pub track_id: TrackId,
    pub clip: Option<PianoRollClipData>,
    pub playhead_beats: f64,
    pub total_beats: f64,
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
        }
    }

    pub fn empty(track_id: TrackId, playhead_beats: f64) -> Self {
        Self {
            track_id,
            clip: None,
            playhead_beats,
            total_beats: 16.0,
        }
    }

    fn beat_to_x(&self, beat: f64, bounds: &Rectangle) -> f32 {
        let grid_width = bounds.width - KEY_WIDTH;
        let total = self.total_beats.max(1.0);
        KEY_WIDTH + (beat / total) as f32 * grid_width
    }

    fn pitch_to_y(pitch: u8) -> f32 {
        let row = (HIGH_NOTE.saturating_sub(pitch).saturating_sub(1)) as f32;
        row * KEY_HEIGHT
    }
}

/// State for piano roll interaction.
#[derive(Debug, Default)]
pub struct PianoRollState {
    // Intentionally empty for now; Default derived for clippy.
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
                Color {
                    r: 0.12,
                    g: 0.12,
                    b: 0.18,
                    a: 1.0,
                }
            } else {
                Color {
                    r: 0.22,
                    g: 0.22,
                    b: 0.30,
                    a: 1.0,
                }
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
                    .with_color(Color {
                        a: 0.2,
                        ..theme::TEXT_DIM
                    })
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
                    r: 0.09,
                    g: 0.09,
                    b: 0.15,
                    a: 1.0,
                }
            } else {
                Color {
                    r: 0.11,
                    g: 0.11,
                    b: 0.18,
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
                    .with_color(Color {
                        a: 0.1,
                        ..theme::TEXT_DIM
                    })
                    .with_width(0.5),
            );
        }

        // Vertical beat grid lines
        let total = self.total_beats.max(1.0);
        let num_beats = total.ceil() as usize;
        for beat in 0..=num_beats {
            let x = KEY_WIDTH + (beat as f32 / total as f32) * grid_width;
            let is_bar = beat % 4 == 0;
            let line_color = if is_bar {
                Color {
                    a: 0.4,
                    ..theme::TEXT_DIM
                }
            } else {
                Color {
                    a: 0.15,
                    ..theme::TEXT_DIM
                }
            };
            let line_width = if is_bar { 1.0 } else { 0.5 };

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
                let bar_num = beat / 4 + 1;
                frame.fill_text(canvas::Text {
                    content: format!("{bar_num}"),
                    position: iced::Point::new(x + 2.0, 1.0),
                    color: Color {
                        a: 0.5,
                        ..theme::TEXT_DIM
                    },
                    size: iced::Pixels(9.0),
                    ..Default::default()
                });
            }
        }

        // Draw notes
        if let Some(ref clip_data) = self.clip {
            for (idx, note) in clip_data.notes.iter().enumerate() {
                if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                    continue;
                }

                let x = self.beat_to_x(note.start_beat, &bounds);
                let y = Self::pitch_to_y(note.pitch);
                let note_w = ((note.duration_beats / total) as f32 * grid_width).max(4.0);
                let note_h = KEY_HEIGHT - 1.0;

                let is_selected = clip_data.selected_note == Some(idx);
                let color = if is_selected {
                    NOTE_SELECTED_COLOR
                } else {
                    NOTE_COLOR
                };

                frame.fill_rectangle(
                    iced::Point::new(x, y + 0.5),
                    iced::Size::new(note_w, note_h),
                    color,
                );

                // Note border
                let note_border = canvas::Path::rectangle(
                    iced::Point::new(x, y + 0.5),
                    iced::Size::new(note_w, note_h),
                );
                frame.stroke(
                    &note_border,
                    canvas::Stroke::default()
                        .with_color(Color {
                            a: 0.5,
                            ..theme::TEXT
                        })
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
                .with_color(theme::TEXT_DIM)
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
            mouse::Interaction::Crosshair
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
        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                // Only handle clicks in the grid area (past the key area)
                if pos.x < KEY_WIDTH {
                    return (canvas::event::Status::Ignored, None);
                }

                let grid_width = bounds.width - KEY_WIDTH;
                let total = self.total_beats.max(1.0);

                // Convert click position to beat and pitch
                let beat = ((pos.x - KEY_WIDTH) / grid_width) as f64 * total;
                let row = (pos.y / KEY_HEIGHT) as u8;
                let pitch = HIGH_NOTE.saturating_sub(1).saturating_sub(row);

                if !(LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                    return (canvas::event::Status::Ignored, None);
                }

                if let Some(ref clip_data) = self.clip {
                    // Check if clicking on an existing note
                    for (idx, note) in clip_data.notes.iter().enumerate() {
                        if note.pitch == pitch
                            && beat >= note.start_beat
                            && beat < note.start_beat + note.duration_beats
                        {
                            // Select the note
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::SelectNote(
                                    self.track_id,
                                    clip_data.clip_id,
                                    Some(idx),
                                )),
                            );
                        }
                    }

                    // Click on empty space: add a note
                    // Snap to nearest beat subdivision
                    let snapped_beat = (beat * 2.0).floor() / 2.0;
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::AddNote {
                            track_id: self.track_id,
                            clip_id: clip_data.clip_id,
                            pitch,
                            start_beat: snapped_beat,
                            duration_beats: DEFAULT_NOTE_DURATION,
                        }),
                    );
                }
            }
        }

        (canvas::event::Status::Ignored, None)
    }
}

/// Returns true if the given MIDI pitch is a black key.
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}
