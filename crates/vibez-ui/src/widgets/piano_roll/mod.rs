//! Piano-roll drawing and pointer editing with optional shared keyboard navigation.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::domains::piano_roll::PianoRollMsg;
use crate::message::Message;
use crate::state::{
    GridConfig, PianoRollEditMode, SnapGrid, UiNoteClip, UndoGestureId, SHORTEST_LENGTH_BEATS,
};
use crate::timeline_geometry::{TimelineGeometry, TimelineViewport};
use crate::timeline_snap::SnapTargets;
use crate::widgets::clip_editor_view::{wheel_intent, ClipEditorView, RulerZoom, WheelIntent};
use crate::widgets::clip_loop_markers::{self, LoopDrag};
use crate::widgets::double_click::DoubleClick;
use crate::widgets::drag_phase::{past_click_slop, DragPhase};
use crate::widgets::local_drag::LocalDrag;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

/// Width of the piano key area on the left.
const KEY_WIDTH: f32 = 52.0;
/// Height of each piano key row.
const KEY_HEIGHT: f32 = 16.0;
/// Height of the ruler strip at the top.
const RULER_HEIGHT: f32 = 30.0;

/// The full MIDI range, as the pads play it: C-1 = 0 up to G9 = 127, the
/// upper bound exclusive.
const LOW_NOTE: u8 = 0;
const HIGH_NOTE: u8 = 128;
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
    pub grid: GridConfig,
    pub scroll_y: f32,
    pub edit_mode: PianoRollEditMode,
    pub reserve_arrow_keys: bool,
    /// The producer's zoom and scroll for this clip; `None` frames its loop.
    pub view: Option<TimelineViewport>,
}

/// Owned data for drawing a note clip in the piano roll.
#[derive(Clone)]
pub struct PianoRollClipData {
    pub clip_id: ClipId,
    pub notes: Vec<MidiNote>,
    pub selected_notes: HashSet<usize>,
    pub start_marker_beats: f64,
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
        grid: GridConfig,
        scroll_y: f32,
        edit_mode: PianoRollEditMode,
    ) -> Self {
        Self {
            track_id,
            clip: Some(PianoRollClipData {
                clip_id: clip.id,
                notes: clip.notes.clone(),
                selected_notes: clip.selected_notes.clone(),
                start_marker_beats: clip.start_marker_beats,
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
            }),
            playhead_beats,
            total_beats,
            track_color,
            grid,
            scroll_y,
            edit_mode,
            reserve_arrow_keys: false,
            view: None,
        }
    }

    pub fn empty(track_id: TrackId, playhead_beats: f64, track_color: Color) -> Self {
        Self {
            track_id,
            clip: None,
            playhead_beats,
            total_beats: 16.0,
            track_color,
            grid: GridConfig::new(SnapGrid::EIGHTH, true, false, 0),
            scroll_y: default_scroll_y(200.0),
            edit_mode: PianoRollEditMode::default(),
            reserve_arrow_keys: false,
            view: None,
        }
    }

    fn clip_view(&self) -> ClipEditorView {
        clip_editor_view(self.clip.as_ref(), self.total_beats, self.view)
    }

    fn geometry(&self, bounds: &Rectangle) -> TimelineGeometry {
        view_geometry(self.clip_view(), bounds.width)
    }

    fn beat_to_x(&self, beat: f64, bounds: &Rectangle) -> f32 {
        self.geometry(bounds).beat_to_x(beat)
    }

    fn x_to_beat(&self, x: f32, bounds: &Rectangle) -> f64 {
        self.geometry(bounds).x_to_beat(x)
    }

    fn pixels_per_beat(&self, bounds: &Rectangle) -> f32 {
        self.geometry(bounds).pixels_per_beat()
    }

    fn effective_grid(&self, bounds: &Rectangle) -> SnapGrid {
        self.grid.effective_grid(self.pixels_per_beat(bounds))
    }

    fn snapped_beat(&self, beat: f64, bounds: &Rectangle) -> f64 {
        self.grid.snap_beat(beat, self.pixels_per_beat(bounds))
    }

    /// Snap for note *creation*: the new note starts in the grid cell the
    /// pointer is over. Nearest-line snapping would drop a click in the
    /// right half of a cell into the following one.
    fn snapped_beat_floor(&self, beat: f64, bounds: &Rectangle) -> f64 {
        self.grid
            .snap_beat_floor(beat, self.pixels_per_beat(bounds))
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
        let geometry = self.geometry(bounds);

        // Reverse order: notes are drawn front-to-back, so the last one
        // painted is the one visually on top and must win the hit.
        for (idx, note) in clip_data.notes.iter().enumerate().rev() {
            if !(LOW_NOTE..HIGH_NOTE).contains(&note.pitch) {
                continue;
            }

            let x = self.beat_to_x(note.start_beat, bounds);
            let y = self.pitch_to_y(note.pitch);
            let note_w = geometry.width_for_beats(note.duration_beats).max(4.0);
            let note_h = KEY_HEIGHT - 1.0;

            if pos.x >= x && pos.x <= x + note_w && pos.y >= y + 0.5 && pos.y <= y + 0.5 + note_h {
                let near_right = pos.x >= x + note_w - RESIZE_HANDLE_PX;
                return Some((idx, near_right));
            }
        }
        None
    }

    /// Musical extent of a rubber-band rectangle, clipped to the grid.
    /// `None` when the box misses the note grid entirely.
    fn marquee_region(
        &self,
        anchor: Point,
        current: Point,
        bounds: &Rectangle,
    ) -> Option<MarqueeRegion> {
        let x0 = anchor.x.min(current.x).max(KEY_WIDTH);
        let x1 = anchor.x.max(current.x);
        let y0 = anchor.y.min(current.y).max(RULER_HEIGHT);
        let y1 = anchor.y.max(current.y);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }

        // Screen y grows downward while pitch grows upward, so the top
        // edge of the box yields the high pitch.
        let high_pitch = self.y_to_pitch(y0);
        let low_pitch = self.y_to_pitch(y1);

        Some(MarqueeRegion {
            start_beat: self.x_to_beat(x0, bounds).max(0.0),
            end_beat: self.x_to_beat(x1, bounds).max(0.0),
            low_pitch: low_pitch.min(high_pitch),
            high_pitch: low_pitch.max(high_pitch),
        })
    }
}

/// Beat span and pitch range covered by a rubber-band rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MarqueeRegion {
    start_beat: f64,
    end_beat: f64,
    low_pitch: u8,
    high_pitch: u8,
}

/// The piano roll and its velocity lane frame the loop, or the whole clip
/// when it does not loop, and scroll as far as the loop reaches.
fn clip_editor_view(
    clip: Option<&PianoRollClipData>,
    total_beats: f64,
    stored: Option<TimelineViewport>,
) -> ClipEditorView {
    let loop_span = clip
        .filter(|clip| clip.loop_enabled)
        .map(|clip| (clip.loop_start_beats, clip.loop_end_beats));
    framed_view(stored, loop_span, total_beats)
}

/// The view of a piano roll showing `total_beats` of content that loops
/// over `loop_span`, unless the producer stored one.
pub(crate) fn framed_view(
    stored: Option<TimelineViewport>,
    loop_span: Option<(f64, f64)>,
    total_beats: f64,
) -> ClipEditorView {
    let total_beats = total_beats.max(1.0);
    ClipEditorView::framing(stored, loop_span, total_beats, total_beats)
}

fn note_area_width(bounds: &Rectangle) -> f32 {
    (bounds.width - KEY_WIDTH).max(1.0)
}

/// The piano roll's zoom for `view` in a widget `width` wide, its key
/// column included.
pub(crate) fn view_geometry(view: ClipEditorView, width: f32) -> TimelineGeometry {
    view.geometry((width - KEY_WIDTH).max(1.0), KEY_WIDTH)
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

pub(crate) use vibez_core::midi::pitch_name;

/// Up and Down move selected notes a semitone, or an octave with Shift.
fn semitone_step(modifiers: iced::keyboard::Modifiers) -> i8 {
    if modifiers.shift() {
        12
    } else {
        1
    }
}

/// A note as it stood when a move or resize began.
#[derive(Debug, Clone, Copy)]
struct MovedNote {
    index: usize,
    pitch: u8,
    start_beat: f64,
    duration_beats: f64,
}

/// The `selection` notes as they stand when a drag begins.
fn grabbed_notes(notes: &[MidiNote], selection: &HashSet<usize>) -> Vec<MovedNote> {
    selection
        .iter()
        .filter_map(|&index| {
            notes.get(index).map(|note| MovedNote {
                index,
                pitch: note.pitch,
                start_beat: note.start_beat,
                duration_beats: note.duration_beats,
            })
        })
        .collect()
}

/// Every resized note's new length: the grabbed note takes `duration` and
/// the rest change by the same amount, none shorter than
/// [`SHORTEST_LENGTH_BEATS`].
fn resized_notes(notes: &[MovedNote], grabbed: usize, duration: f64) -> Vec<(usize, f64)> {
    let delta = notes
        .iter()
        .find(|note| note.index == grabbed)
        .map_or(0.0, |note| duration - note.duration_beats);
    notes
        .iter()
        .map(|note| {
            let resized = if note.index == grabbed {
                duration
            } else {
                (note.duration_beats + delta).max(SHORTEST_LENGTH_BEATS)
            };
            (note.index, resized)
        })
        .collect()
}

/// Where every moved note lands for an offset of `beats` and `semitones`,
/// the offset first held back so the whole group stays inside the clip's
/// `total_beats` and the MIDI range and keeps its shape.
fn moved_notes(
    notes: &[MovedNote],
    beats: f64,
    semitones: i16,
    total_beats: f64,
) -> Vec<(usize, f64, u8)> {
    let earliest = notes
        .iter()
        .map(|note| note.start_beat)
        .fold(f64::MAX, f64::min);
    let latest_end = notes
        .iter()
        .map(|note| note.start_beat + note.duration_beats)
        .fold(f64::MIN, f64::max);
    let lowest = notes.iter().map(|note| note.pitch).min().unwrap_or(0) as i16;
    let highest = notes.iter().map(|note| note.pitch).max().unwrap_or(0) as i16;
    let beats = beats.clamp(-earliest, (total_beats - latest_end).max(-earliest));
    let semitones = semitones.clamp(LOW_NOTE as i16 - lowest, HIGH_NOTE as i16 - 1 - highest);
    notes
        .iter()
        .map(|note| {
            (
                note.index,
                note.start_beat + beats,
                (note.pitch as i16 + semitones) as u8,
            )
        })
        .collect()
}

/// Drag action in progress. Each note drag remembers what it last sent, so
/// a pointer move that changes nothing sends no edit and leaves no undo step.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
enum DragAction {
    /// Moving the grabbed note and every note selected with it, together.
    MoveNote {
        original_start_beat: f64,
        original_notes: Vec<MovedNote>,
        start_x: f32,
        start_y: f32,
        undo_gesture: UndoGestureId,
        phase: DragPhase,
        sent_moves: Vec<(usize, f64, u8)>,
    },
    /// Resizing the grabbed note and every note selected with it, together.
    ResizeNote {
        note_index: usize,
        original_duration: f64,
        original_notes: Vec<MovedNote>,
        start_x: f32,
        undo_gesture: UndoGestureId,
        phase: DragPhase,
        sent_duration: f64,
    },
    DrawNote {
        clip_id: ClipId,
        pitch: u8,
        start_beat: f64,
        start_x: f32,
        undo_gesture: UndoGestureId,
        sent_duration: f64,
    },
    /// Rubber-band select dragged out from empty grid space.
    MarqueeNotes {
        anchor: Point,
        current: Point,
        additive: bool,
    },
    Loop {
        clip_id: ClipId,
        drag: LoopDrag,
    },
    Start {
        clip_id: ClipId,
        undo_gesture: UndoGestureId,
    },
    Ruler {
        clip_id: ClipId,
        zoom: RulerZoom,
    },
}

/// Max time between clicks to count as double-click (ms).
const DOUBLE_CLICK_MS: u64 = 400;
/// Max distance between clicks to count as double-click (px).
const DOUBLE_CLICK_DIST: f32 = 8.0;

/// State for piano roll interaction.
#[derive(Debug, Default)]
pub struct PianoRollState {
    drag: Option<DragAction>,
    last_cursor: Option<Point>,
    shift_held: bool,
    alt_held: bool,
    double_click: DoubleClick,
    /// Pitch currently held down via a key-lane click (audition).
    audition_pitch: Option<u8>,
}

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
                DragAction::Loop {
                    drag: LoopDrag::Move { .. },
                    ..
                } => mouse::Interaction::Grabbing,
                DragAction::Loop { .. } | DragAction::Start { .. } => {
                    mouse::Interaction::ResizingHorizontally
                }
                DragAction::MarqueeNotes { .. } => mouse::Interaction::Crosshair,
                DragAction::Ruler { .. } => mouse::Interaction::ZoomIn,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            if self.hit_test_start_marker(pos, &bounds) {
                return mouse::Interaction::ResizingHorizontally;
            }
            if let Some(marker) = self.hit_test_loop_marker(pos, &bounds) {
                return marker.interaction(false);
            }
        }

        // Draw mode: the note grid uses a crosshair, but loop braces
        // retain their horizontal resize cursor in every edit mode.
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
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let clip_id = self.clip.as_ref().map(|clip| clip.clip_id);
                    let width = note_area_width(&bounds);
                    let anchor_x = cursor
                        .position_in(bounds)
                        .map_or(width / 2.0, |position| position.x - KEY_WIDTH);
                    let viewport = match wheel_intent(delta, state.shift_held) {
                        WheelIntent::Zoom(factor) => {
                            Some(self.clip_view().zoomed(factor, anchor_x, width))
                        }
                        WheelIntent::ScrollTime(pixels) => {
                            Some(self.clip_view().scrolled(pixels, width))
                        }
                        WheelIntent::Plain(_) => None,
                    };
                    if let Some(viewport) = viewport {
                        return (
                            canvas::event::Status::Captured,
                            clip_id.map(|clip_id| Message::set_clip_editor_view(clip_id, viewport)),
                        );
                    }
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
                    if pos.y <= RULER_HEIGHT {
                        let Some(drag) = self
                            .begin_marker_drag(pos, &bounds, state.alt_held)
                            .or_else(|| {
                                self.clip.as_ref().map(|clip| DragAction::Ruler {
                                    clip_id: clip.clip_id,
                                    zoom: RulerZoom::begin(
                                        self.clip_view(),
                                        Point::new(pos.x - KEY_WIDTH, pos.y),
                                        note_area_width(&bounds),
                                    ),
                                })
                            })
                        else {
                            return (canvas::event::Status::Ignored, None);
                        };
                        state.drag = Some(drag);
                        state.last_cursor = Some(pos);
                        let pin = self.clip.as_ref().and_then(|clip| {
                            let viewport = self.clip_view().pin(note_area_width(&bounds))?;
                            Some(Message::set_clip_editor_view(clip.clip_id, viewport))
                        });
                        return (canvas::event::Status::Captured, pin);
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

                            let note_duration = self.effective_grid(&bounds).beat_size();
                            let snapped_beat = self.snapped_beat_floor(beat, &bounds).max(0.0);

                            let max_start = self.total_beats - note_duration;
                            if max_start < 0.0 || snapped_beat > max_start {
                                return (canvas::event::Status::Captured, None);
                            }

                            let undo_gesture = UndoGestureId::new();
                            state.drag = Some(DragAction::DrawNote {
                                clip_id: clip_data.clip_id,
                                pitch,
                                start_beat: snapped_beat,
                                start_x: pos.x,
                                undo_gesture,
                                sent_duration: note_duration,
                            });

                            return (
                                canvas::event::Status::Captured,
                                Some(
                                    Message::PianoRoll(PianoRollMsg::AddNote {
                                        track_id: self.track_id,
                                        clip_id: clip_data.clip_id,
                                        pitch,
                                        start_beat: snapped_beat,
                                        duration_beats: note_duration,
                                    })
                                    .in_undo_gesture(undo_gesture),
                                ),
                            );
                        }

                        // ── Select mode (default) ──
                        // Hit test existing notes
                        if let Some((idx, near_right)) = self.hit_test_note(pos, &bounds) {
                            let note = &clip_data.notes[idx];
                            let shift = state.shift_held;

                            if near_right {
                                let selected = clip_data.selected_notes.contains(&idx);
                                let resized = if selected {
                                    clip_data.selected_notes.clone()
                                } else {
                                    HashSet::from([idx])
                                };
                                state.drag = Some(DragAction::ResizeNote {
                                    note_index: idx,
                                    original_duration: note.duration_beats,
                                    original_notes: grabbed_notes(&clip_data.notes, &resized),
                                    start_x: pos.x,
                                    undo_gesture: UndoGestureId::new(),
                                    phase: DragPhase::Pending,
                                    sent_duration: note.duration_beats,
                                });
                                return (
                                    canvas::event::Status::Captured,
                                    (!selected).then(|| {
                                        Message::PianoRoll(PianoRollMsg::SelectNote(
                                            self.track_id,
                                            clip_data.clip_id,
                                            Some(idx),
                                            false,
                                        ))
                                    }),
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

                                let original_notes = grabbed_notes(&clip_data.notes, &selected_set);

                                let sent_moves = original_notes
                                    .iter()
                                    .map(|note| (note.index, note.start_beat, note.pitch))
                                    .collect();
                                state.drag = Some(DragAction::MoveNote {
                                    original_start_beat: note.start_beat,
                                    original_notes,
                                    start_x: pos.x,
                                    start_y: pos.y,
                                    undo_gesture: UndoGestureId::new(),
                                    phase: DragPhase::Pending,
                                    sent_moves,
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
                        // Double-click → add note, single-click → deselect.
                        // The press stays on record (no clear) so a
                        // rapid third click adds another note.
                        let is_double = state.double_click.press(
                            Instant::now(),
                            pos,
                            Duration::from_millis(DOUBLE_CLICK_MS),
                            Some(DOUBLE_CLICK_DIST),
                        );

                        if is_double {
                            // Double-click: add note
                            let beat = self.x_to_beat(pos.x, &bounds);
                            let pitch = self.y_to_pitch(pos.y);

                            if !(LOW_NOTE..HIGH_NOTE).contains(&pitch) {
                                return (canvas::event::Status::Ignored, None);
                            }

                            let note_duration = self.effective_grid(&bounds).beat_size();
                            let snapped_beat = self.snapped_beat_floor(beat, &bounds).max(0.0);

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

                        // Single-click: begin a rubber-band. A press that
                        // never travels stays a plain deselect.
                        state.drag = Some(DragAction::MarqueeNotes {
                            anchor: pos,
                            current: pos,
                            additive: state.shift_held,
                        });

                        if state.shift_held {
                            // Shift extends the existing selection, so the
                            // press must not clear it up front.
                            return (canvas::event::Status::Captured, None);
                        }

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
                if let Some(local) = LocalDrag::unclamped().position(cursor, bounds) {
                    state.last_cursor = Some(local);

                    if let Some(DragAction::Ruler { clip_id, zoom }) = state.drag {
                        let viewport = zoom.drag_to(
                            Point::new(local.x - KEY_WIDTH, local.y),
                            note_area_width(&bounds),
                        );
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::set_clip_editor_view(clip_id, viewport)),
                        );
                    }

                    if let Some(drag) = state.drag.as_ref() {
                        if let Some(message) =
                            self.marker_drag_message(drag, local, &bounds, state.alt_held)
                        {
                            return (canvas::event::Status::Captured, Some(message));
                        }
                    }

                    // The rubber-band only grows its rectangle; the
                    // selection is resolved once, on release.
                    if let Some(DragAction::MarqueeNotes { current, .. }) = state.drag.as_mut() {
                        *current = local;
                        return (canvas::event::Status::Captured, None);
                    }

                    match state.drag.as_mut() {
                        Some(DragAction::MoveNote {
                            start_x,
                            start_y,
                            phase,
                            ..
                        }) => {
                            if !phase.follow(Point::new(*start_x, *start_y), local) {
                                return (canvas::event::Status::Captured, None);
                            }
                        }
                        Some(DragAction::ResizeNote { start_x, phase, .. }) => {
                            if !phase.follow_x(*start_x, local.x) {
                                return (canvas::event::Status::Captured, None);
                            }
                        }
                        _ => {}
                    }

                    if let Some(drag) = state.drag.as_mut() {
                        if let Some(ref clip_data) = self.clip {
                            let geometry = self.geometry(&bounds);

                            match drag {
                                DragAction::MoveNote {
                                    original_start_beat,
                                    original_notes,
                                    start_x,
                                    start_y,
                                    undo_gesture,
                                    sent_moves,
                                    ..
                                } => {
                                    let dx = local.x - *start_x;
                                    let dy = local.y - *start_y;
                                    let beat_delta = geometry.beats_for_width(dx);
                                    let pitch_delta = -(dy / KEY_HEIGHT).round() as i16;
                                    // The grabbed note snaps; the rest keep their
                                    // places against it.
                                    let snapped_beat_delta = self
                                        .snapped_beat(*original_start_beat + beat_delta, &bounds)
                                        - *original_start_beat;
                                    let moves = moved_notes(
                                        original_notes,
                                        snapped_beat_delta,
                                        pitch_delta,
                                        self.total_beats,
                                    );
                                    if moves == *sent_moves {
                                        return (canvas::event::Status::Captured, None);
                                    }
                                    sent_moves.clone_from(&moves);
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(
                                            Message::PianoRoll(PianoRollMsg::MoveNotesAbsolute {
                                                track_id: self.track_id,
                                                clip_id: clip_data.clip_id,
                                                moves,
                                            })
                                            .in_undo_gesture(*undo_gesture),
                                        ),
                                    );
                                }
                                DragAction::ResizeNote {
                                    note_index,
                                    original_duration,
                                    original_notes,
                                    start_x,
                                    undo_gesture,
                                    sent_duration,
                                    ..
                                } => {
                                    let beat_delta = geometry.beats_for_width(local.x - *start_x);
                                    let start_beat = original_notes
                                        .iter()
                                        .find(|note| note.index == *note_index)
                                        .map_or(0.0, |note| note.start_beat);
                                    let context =
                                        self.grid.snap_context(geometry.pixels_per_beat(), false);
                                    let new_duration = SnapTargets::default()
                                        .span_end(
                                            start_beat,
                                            start_beat + *original_duration + beat_delta,
                                            context,
                                        )
                                        .beat
                                        - start_beat;
                                    if new_duration == *sent_duration {
                                        return (canvas::event::Status::Captured, None);
                                    }
                                    *sent_duration = new_duration;
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(
                                            Message::PianoRoll(PianoRollMsg::SetNoteDurations {
                                                track_id: self.track_id,
                                                clip_id: clip_data.clip_id,
                                                durations: resized_notes(
                                                    original_notes,
                                                    *note_index,
                                                    new_duration,
                                                ),
                                            })
                                            .in_undo_gesture(*undo_gesture),
                                        ),
                                    );
                                }
                                DragAction::DrawNote {
                                    clip_id,
                                    pitch,
                                    start_beat,
                                    start_x,
                                    undo_gesture,
                                    sent_duration,
                                } => {
                                    // Extend note duration while dragging
                                    let dx = local.x - *start_x;
                                    let beat_delta = geometry.beats_for_width(dx);
                                    let min_duration = self.effective_grid(&bounds).beat_size();
                                    let new_duration = self
                                        .snapped_beat(
                                            (min_duration + beat_delta.max(0.0)).max(min_duration),
                                            &bounds,
                                        )
                                        .max(if self.grid.snap_enabled {
                                            min_duration
                                        } else {
                                            0.01
                                        });

                                    // Find the note we just added (last note at this pitch/beat)
                                    if let Some(idx) = clip_data.notes.iter().rposition(|n| {
                                        n.pitch == *pitch
                                            && (n.start_beat - *start_beat).abs() < 0.001
                                    }) {
                                        if new_duration == *sent_duration {
                                            return (canvas::event::Status::Captured, None);
                                        }
                                        *sent_duration = new_duration;
                                        let mut note = clip_data.notes[idx];
                                        note.duration_beats = new_duration;
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(
                                                Message::PianoRoll(PianoRollMsg::EditNote(
                                                    self.track_id,
                                                    *clip_id,
                                                    idx,
                                                    note,
                                                ))
                                                .in_undo_gesture(*undo_gesture),
                                            ),
                                        );
                                    }
                                }
                                // Handled above, before this borrow.
                                DragAction::MarqueeNotes { .. }
                                | DragAction::Loop { .. }
                                | DragAction::Start { .. }
                                | DragAction::Ruler { .. } => {}
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
                    if let DragAction::MarqueeNotes {
                        anchor,
                        current,
                        additive,
                    } = drag
                    {
                        let travelled = past_click_slop(anchor, current);
                        if let (true, Some(clip_data)) = (travelled, self.clip.as_ref()) {
                            if let Some(region) = self.marquee_region(anchor, current, &bounds) {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::PianoRoll(PianoRollMsg::SelectNotesInRegion {
                                        track_id: self.track_id,
                                        clip_id: clip_data.clip_id,
                                        start_beat: region.start_beat,
                                        end_beat: region.end_beat,
                                        low_pitch: region.low_pitch,
                                        high_pitch: region.high_pitch,
                                        additive,
                                    })),
                                );
                            }
                        }
                        return (canvas::event::Status::Captured, None);
                    }

                    return (canvas::event::Status::Captured, None);
                }
            }

            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
                state.alt_held = modifiers.alt();
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
            }) if !self.reserve_arrow_keys => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: -self.effective_grid(&bounds).beat_size(),
                                delta_semitones: 0,
                            })),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight),
                ..
            }) if !self.reserve_arrow_keys => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: self.effective_grid(&bounds).beat_size(),
                                delta_semitones: 0,
                            })),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp),
                modifiers,
                ..
            }) if !self.reserve_arrow_keys => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: semitone_step(modifiers),
                            })),
                        );
                    }
                }
            }
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
                modifiers,
                ..
            }) if !self.reserve_arrow_keys => {
                if let Some(ref clip_data) = self.clip {
                    if !clip_data.selected_notes.is_empty() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::PianoRoll(PianoRollMsg::NudgeSelectedNotes {
                                track_id: self.track_id,
                                clip_id: clip_data.clip_id,
                                delta_beats: 0.0,
                                delta_semitones: -semitone_step(modifiers),
                            })),
                        );
                    }
                }
            }

            // Command+A is handled centrally by the global
            // SelectAllPressed shortcut, for the same reason Delete is:
            // every canvas receives keyboard events, so two canvases
            // binding one key race each other.
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
mod loop_drag;
#[cfg(test)]
mod note_gesture_tests;
#[cfg(test)]
mod tests;
mod velocity_lane;
#[cfg(test)]
mod view_tests;

pub use velocity_lane::VelocityLaneWidget;
