//! Note gestures driven through the piano roll widget, the piano roll domain
//! and the undo history together, the way the running app wires them.

use std::sync::Arc;

use super::*;
use crate::domains::piano_roll::PianoRollCtx;
use crate::domains::DiscardingEngine;
use crate::state::{AppState, ProjectSnapshot, TimelineEditorState, UndoHistory};
use iced::widget::canvas::Program;

const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 852.0,
    height: 400.0,
};
const CLIP_BEATS: f64 = 8.0;

fn note(pitch: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

/// One open note clip, rebuilt into a fresh widget for every pointer event
/// as iced rebuilds the view, while the widget state lives across them.
struct Editor {
    track_id: TrackId,
    clip_id: ClipId,
    edit_mode: PianoRollEditMode,
    timeline: TimelineEditorState,
    domain: crate::state::PianoRollState,
    history: UndoHistory,
    snapshot: ProjectSnapshot,
    pointer: PianoRollState,
}

impl Editor {
    fn new(notes: Vec<MidiNote>, selected_notes: HashSet<usize>) -> Self {
        let track_id = TrackId::new();
        let clip_id = ClipId::new();
        let mut timeline = TimelineEditorState::default();
        Arc::make_mut(&mut timeline.timeline)
            .ensure(track_id)
            .note_clips
            .push(UiNoteClip {
                id: clip_id,
                name: "Notes".into(),
                position_beats: 0.0,
                duration_beats: CLIP_BEATS,
                notes,
                selected_notes,
                start_marker_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: CLIP_BEATS,
                groove_grid: vibez_core::perform::GrooveGrid::Off,
            });
        Self {
            track_id,
            clip_id,
            edit_mode: PianoRollEditMode::Select,
            timeline,
            domain: Default::default(),
            history: UndoHistory::default(),
            snapshot: AppState::default().project_snapshot(),
            pointer: PianoRollState::default(),
        }
    }

    fn clip(&self) -> &UiNoteClip {
        self.timeline
            .timeline
            .get(self.track_id)
            .and_then(|track| track.note_clips.iter().find(|c| c.id == self.clip_id))
            .expect("the edited clip")
    }

    fn notes(&self) -> Vec<MidiNote> {
        self.clip().notes.clone()
    }

    fn widget(&self) -> PianoRollWidget {
        PianoRollWidget::from_clip(
            self.track_id,
            self.clip(),
            0.0,
            CLIP_BEATS,
            Color::WHITE,
            GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
            default_scroll_y(BOUNDS.height),
            self.edit_mode,
        )
    }

    /// Where `beat` falls on the row of `pitch`.
    fn at(&self, beat: f64, pitch: u8) -> Point {
        let widget = self.widget();
        Point::new(
            widget.beat_to_x(beat, &BOUNDS),
            widget.pitch_to_y(pitch) + KEY_HEIGHT / 2.0,
        )
    }

    fn send(&mut self, event: mouse::Event, at: Point) {
        let widget = self.widget();
        let (_, message) = widget.update(
            &mut self.pointer,
            canvas::Event::Mouse(event),
            BOUNDS,
            mouse::Cursor::Available(at),
        );
        let Some(message) = message else {
            return;
        };
        let (message, gesture) = match message {
            Message::UndoGesture { id, edit } => (*edit, Some(id)),
            message => (message, None),
        };
        let Message::PianoRoll(edit) = message else {
            return;
        };
        if edit.marks_dirty() {
            let before = ProjectSnapshot {
                arrange_timeline: Arc::clone(&self.timeline.timeline),
                ..self.snapshot.clone()
            };
            self.history.push_edit(before, gesture);
        }
        self.domain.update(
            edit,
            &mut DiscardingEngine,
            &mut self.timeline,
            PianoRollCtx::default(),
        );
    }

    /// Press at `from`, move through `path`, release at its last point.
    fn drag(&mut self, from: Point, path: &[Point]) {
        self.send(mouse::Event::ButtonPressed(mouse::Button::Left), from);
        for &position in path {
            self.send(mouse::Event::CursorMoved { position }, position);
        }
        let end = path.last().copied().unwrap_or(from);
        self.send(mouse::Event::ButtonReleased(mouse::Button::Left), end);
    }

    fn undo(&mut self) {
        let before = self.history.pop_undo().expect("an undo step");
        self.timeline.timeline = before.arrange_timeline;
    }
}

/// Pointer positions one pixel apart from `from` to `to`, as a slow hand
/// produces them.
fn steps(from: Point, to: Point) -> Vec<Point> {
    let count = (to.x - from.x).abs().max((to.y - from.y).abs()).ceil() as usize;
    (1..=count)
        .map(|step| {
            let t = step as f32 / count as f32;
            Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
        })
        .collect()
}

#[test]
fn a_long_note_resize_drag_is_one_undo_step() {
    let original = vec![note(60, 0.0, 1.0)];
    let mut editor = Editor::new(original.clone(), HashSet::new());
    let grab = Point::new(editor.at(1.0, 60).x - 2.0, editor.at(1.0, 60).y);
    let release = Point::new(editor.at(3.0, 60).x - 2.0, grab.y);

    editor.drag(grab, &steps(grab, release));

    assert_eq!(editor.notes()[0].duration_beats, 3.0);
    assert_eq!(editor.history.undo.len(), 1);
    editor.undo();
    assert_eq!(editor.notes(), original);
}

#[test]
fn drawing_a_note_by_dragging_is_one_undo_step() {
    let mut editor = Editor::new(Vec::new(), HashSet::new());
    editor.edit_mode = PianoRollEditMode::Draw;
    let press = editor.at(1.05, 62);
    let release = editor.at(2.05, 62);

    editor.drag(press, &steps(press, release));

    assert_eq!(editor.notes(), vec![note(62, 1.0, 1.25)]);
    assert_eq!(editor.history.undo.len(), 1);
    editor.undo();
    assert!(editor.notes().is_empty());
}

#[test]
fn a_shaky_click_on_an_off_grid_note_end_leaves_it_alone() {
    let original = vec![note(60, 0.1, 0.93)];
    let mut editor = Editor::new(original.clone(), HashSet::new());
    let grab = Point::new(editor.at(1.03, 60).x - 1.0, editor.at(1.03, 60).y);

    editor.drag(grab, &[Point::new(grab.x + 2.0, grab.y + 1.0)]);

    assert_eq!(editor.notes(), original);
    assert!(editor.history.undo.is_empty());
}

#[test]
fn a_shaky_click_on_an_off_grid_note_body_leaves_it_alone() {
    let original = vec![note(60, 0.1, 0.93)];
    let mut editor = Editor::new(original.clone(), HashSet::new());
    let grab = editor.at(0.5, 60);

    editor.drag(grab, &[Point::new(grab.x + 2.0, grab.y + 2.0)]);

    assert_eq!(editor.notes(), original);
    assert!(editor.history.undo.is_empty());
}

#[test]
fn a_note_click_that_wobbles_by_exactly_the_slop_stays_a_click_as_on_the_lanes() {
    let original = vec![note(60, 0.1, 0.93)];
    let mut editor = Editor::new(original.clone(), HashSet::new());
    let body = editor.at(0.5, 60);
    let end = Point::new(editor.at(1.03, 60).x - 1.0, body.y);
    let slop = crate::widgets::drag_phase::CLICK_SLOP_PX;

    editor.drag(body, &[Point::new(body.x + slop, body.y + slop)]);
    editor.drag(end, &[Point::new(end.x + slop, end.y)]);

    assert_eq!(editor.notes(), original);
    assert!(editor.history.undo.is_empty());
}

#[test]
fn note_drags_that_stay_in_their_grid_cell_leave_no_undo_step() {
    let original = vec![note(60, 1.0, 1.0)];
    let mut editor = Editor::new(original.clone(), HashSet::new());
    let body = editor.at(1.5, 60);
    let end = Point::new(editor.at(2.0, 60).x - 2.0, body.y);

    editor.drag(body, &steps(body, Point::new(body.x + 10.0, body.y)));
    editor.drag(end, &steps(end, Point::new(end.x + 10.0, end.y)));

    assert_eq!(editor.notes(), original);
    assert!(editor.history.undo.is_empty());
}

fn durations(editor: &Editor) -> Vec<f64> {
    editor
        .notes()
        .iter()
        .map(|note| note.duration_beats)
        .collect()
}

#[test]
fn resizing_one_selected_note_resizes_the_whole_selection_in_one_undo_step() {
    let original = vec![note(60, 0.0, 1.0), note(64, 2.0, 0.5), note(67, 4.0, 1.0)];
    let mut editor = Editor::new(original.clone(), HashSet::from([0, 1]));
    let grab = Point::new(editor.at(1.0, 60).x - 2.0, editor.at(1.0, 60).y);
    let release = Point::new(editor.at(2.0, 60).x - 2.0, grab.y);

    editor.drag(grab, &steps(grab, release));

    assert_eq!(durations(&editor), vec![2.0, 1.5, 1.0]);
    assert_eq!(editor.clip().selected_notes, HashSet::from([0, 1]));
    assert_eq!(editor.history.undo.len(), 1);
    editor.undo();
    assert_eq!(editor.notes(), original);
}

#[test]
fn shrinking_a_selection_keeps_every_note_at_least_the_shortest_length() {
    let original = vec![note(60, 0.0, 1.0), note(64, 2.0, 0.5)];
    let mut editor = Editor::new(original, HashSet::from([0, 1]));
    let grab = Point::new(editor.at(1.0, 60).x - 2.0, editor.at(1.0, 60).y);
    let release = Point::new(editor.at(0.5, 60).x - 2.0, grab.y);

    editor.drag(grab, &steps(grab, release));

    assert_eq!(durations(&editor), vec![0.5, SHORTEST_LENGTH_BEATS]);
}

#[test]
fn resizing_an_unselected_note_selects_and_resizes_only_it() {
    let original = vec![note(60, 0.0, 1.0), note(64, 2.0, 0.5)];
    let mut editor = Editor::new(original, HashSet::from([1]));
    let grab = Point::new(editor.at(1.0, 60).x - 2.0, editor.at(1.0, 60).y);
    let release = Point::new(editor.at(2.0, 60).x - 2.0, grab.y);

    editor.drag(grab, &steps(grab, release));

    assert_eq!(durations(&editor), vec![2.0, 0.5]);
    assert_eq!(editor.clip().selected_notes, HashSet::from([0]));
}

#[test]
fn a_rubber_band_of_exactly_the_slop_stays_a_click_as_on_the_lanes() {
    let slop = crate::widgets::drag_phase::CLICK_SLOP_PX;
    let band = |travel: f32| {
        let mut editor = Editor::new(vec![note(60, 1.0, 1.0)], HashSet::new());
        let note_start = editor.at(1.0, 60);
        let press = Point::new(note_start.x - slop / 2.0, note_start.y);
        editor.drag(press, &[Point::new(press.x + travel, press.y + slop)]);
        editor.clip().selected_notes.clone()
    };

    assert!(band(slop).is_empty());
    assert_eq!(band(slop + 1.0), HashSet::from([0]));
}
