//! A Clip drag records an undo step and dirties the project only when it
//! changes the Clip.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiNoteClip, UndoGestureId};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::GrooveGrid;

/// One selected note Clip at beat 1.1 on a MIDI track.
fn off_grid_clip() -> (App, TrackId, ClipId) {
    let mut app = super::test_support::app();
    let mut track = ProjectTrack::new(TrackId::new(), "MIDI".into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let clip_id = ClipId::new();
    app.state
        .arrange_content_mut(track_id)
        .note_clips
        .push(UiNoteClip {
            id: clip_id,
            name: "Capture".into(),
            position_beats: 1.1,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
            groove_grid: GrooveGrid::Off,
        });
    app.state
        .arrangement
        .selected_clips
        .insert(ArrangementSelection::NoteClip { track_id, clip_id });
    (app, track_id, clip_id)
}

fn drag(app: &mut App, gesture: UndoGestureId, message: ArrangementMsg) {
    let _ = app.update(Message::Arrangement(message).in_undo_gesture(gesture));
}

#[test]
fn a_move_and_resize_that_change_nothing_leave_no_undo_step() {
    let (mut app, track_id, clip_id) = off_grid_clip();

    drag(
        &mut app,
        UndoGestureId::new(),
        ArrangementMsg::MoveSelectedClips {
            anchor: clip_id,
            new_position_beats: 1.1,
            target_track: track_id,
        },
    );
    drag(
        &mut app,
        UndoGestureId::new(),
        ArrangementMsg::ResizeSelectedClips {
            anchor: ArrangementSelection::NoteClip { track_id, clip_id },
            new_duration_beats: 4.0,
        },
    );

    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);
}

#[test]
fn a_left_edge_trim_is_one_undo_step() {
    let (mut app, track_id, clip_id) = off_grid_clip();
    let anchor = ArrangementSelection::NoteClip { track_id, clip_id };
    let gesture = UndoGestureId::new();

    for new_start_beats in [1.5, 2.0] {
        drag(
            &mut app,
            gesture,
            ArrangementMsg::TrimSelectedClipsStart {
                anchor,
                new_start_beats,
            },
        );
    }
    assert!(app.state.project.dirty);
    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));

    let clip = &app.state.arrange_content_mut(track_id).note_clips[0];
    assert_eq!((clip.position_beats, clip.duration_beats), (1.1, 4.0));
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn a_left_edge_trim_that_changes_nothing_leaves_no_undo_step() {
    let (mut app, track_id, clip_id) = off_grid_clip();

    drag(
        &mut app,
        UndoGestureId::new(),
        ArrangementMsg::TrimSelectedClipsStart {
            anchor: ArrangementSelection::NoteClip { track_id, clip_id },
            new_start_beats: 1.1,
        },
    );

    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);
}

#[test]
fn a_move_and_the_overwrite_on_its_drop_are_one_undo_step() {
    let (mut app, track_id, clip_id) = off_grid_clip();
    let mut neighbour = app.state.arrange_content_mut(track_id).note_clips[0].clone();
    neighbour.id = ClipId::new();
    neighbour.position_beats = 6.0;
    app.state
        .arrange_content_mut(track_id)
        .note_clips
        .push(neighbour);
    let gesture = UndoGestureId::new();

    drag(
        &mut app,
        gesture,
        ArrangementMsg::MoveSelectedClips {
            anchor: clip_id,
            new_position_beats: 5.0,
            target_track: track_id,
        },
    );
    drag(
        &mut app,
        gesture,
        ArrangementMsg::DropClips { anchor: clip_id },
    );
    let spans = |app: &mut App| {
        let mut spans: Vec<_> = app
            .state
            .arrange_content_mut(track_id)
            .note_clips
            .iter()
            .map(|clip| (clip.position_beats, clip.duration_beats))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        spans
    };
    assert_eq!(spans(&mut app), vec![(5.0, 4.0), (9.0, 1.0)]);

    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));

    assert_eq!(spans(&mut app), vec![(1.1, 4.0), (6.0, 4.0)]);
    assert!(!app.state.project.history.can_undo());
}
