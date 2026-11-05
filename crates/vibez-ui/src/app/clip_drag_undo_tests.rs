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
