use std::collections::HashSet;
use std::time::Instant;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::domains::piano_roll::PianoRollMsg;
use crate::message::Message;
use crate::state::{PianoRollEditMode, SnapGrid, UiNoteClip};
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
pub(crate) fn pitch_name(pitch: u8) -> String {
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
    /// Pitch currently held down via a key-lane click (audition).
    audition_pitch: Option<u8>,
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
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        self.draw_impl(state, renderer, bounds, cursor)
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
                        Some(Message::PianoRoll(PianoRollMsg::ScrollY(new_scroll))),
                    );
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Ignore clicks in ruler area
                    if pos.y < RULER_HEIGHT {
                        return (canvas::event::Status::Ignored, None);
                    }

                    // Key lane: press auditions the pitch on this
                    // track's instrument (released on mouse-up).
                    if pos.x < KEY_WIDTH {
                        let pitch = self.y_to_pitch(pos.y);
                        if (LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                            state.audition_pitch = Some(pitch);
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::audition_note(self.track_id, pitch, true)),
                            );
                        }
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
                                    Some(Message::PianoRoll(PianoRollMsg::RemoveNote(
                                        self.track_id,
                                        clip_data.clip_id,
                                        idx,
                                    ))),
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
                                Some(Message::PianoRoll(PianoRollMsg::AddNote {
                                    track_id: self.track_id,
                                    clip_id: clip_data.clip_id,
                                    pitch,
                                    start_beat: snapped_beat,
                                    duration_beats: note_duration,
                                })),
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
                                    Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                                        self.track_id,
                                        clip_data.clip_id,
                                        Some(idx),
                                        false,
                                    ))),
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
                                Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                                    self.track_id,
                                    clip_data.clip_id,
                                    Some(idx),
                                    shift,
                                )))
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
                                Some(Message::PianoRoll(PianoRollMsg::AddNote {
                                    track_id: self.track_id,
                                    clip_id: clip_data.clip_id,
                                    pitch,
                                    start_beat: snapped_beat,
                                    duration_beats: note_duration,
                                })),
                            );
                        }

                        // Single-click: deselect all
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                                self.track_id,
                                clip_data.clip_id,
                                None,
                                false,
                            ))),
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
                                            Some(Message::PianoRoll(PianoRollMsg::EditNote(
                                                self.track_id,
                                                clip_data.clip_id,
                                                idx,
                                                note,
                                            ))),
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
                                            Some(Message::PianoRoll(PianoRollMsg::EditNote(
                                                self.track_id,
                                                clip_data.clip_id,
                                                idx,
                                                note,
                                            ))),
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
                                            Some(Message::PianoRoll(PianoRollMsg::EditNote(
                                                self.track_id,
                                                *clip_id,
                                                idx,
                                                note,
                                            ))),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // Release an auditioned key first: it never coexists
                // with a note drag.
                if let Some(pitch) = state.audition_pitch.take() {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::audition_note(self.track_id, pitch, false)),
                    );
                }
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
                                            Some(Message::PianoRoll(
                                                PianoRollMsg::MoveNotesAbsolute {
                                                    track_id: self.track_id,
                                                    clip_id: clip_data.clip_id,
                                                    moves,
                                                },
                                            )),
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

            // Delete/Backspace are handled centrally by the global
            // DeleteKeyPressed shortcut (context-aware, respects text
            // input focus). Canvases must NOT bind them: every canvas
            // receives keyboard events, so two canvases binding the
            // same key race each other (the timeline used to win and
            // delete the clip while the piano roll wanted the note).

            // ── Keyboard: Arrow keys → nudge selected notes ──
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft),
                ..
            }) => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: -self.snap_grid.beat_size(),
                                delta_semitones: 0,
                            })),
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
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: self.snap_grid.beat_size(),
                                delta_semitones: 0,
                            })),
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
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: 1,
                            })),
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
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: -1,
                            })),
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
                        Some(Message::PianoRoll(PianoRollMsg::SelectAllNotes(
                            self.track_id,
                            clip_data.clip_id,
                        ))),
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

mod draw;
