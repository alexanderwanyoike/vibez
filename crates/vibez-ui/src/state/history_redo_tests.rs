//! Redo traversal preserves future snapshots until a new edit forks history.

use crate::domains::project::{ProjectCtx, ProjectMsg};
use crate::state::AppState;

fn edit_tempo(state: &mut AppState, bpm: f64) {
    let before = state.project_snapshot();
    state.project.history.push_edit(before, None);
    state.transport.bpm = bpm;
}

fn apply_history(state: &mut AppState, message: ProjectMsg) -> bool {
    let context = ProjectCtx {
        snapshot_now: state.project_snapshot(),
    };
    let action = state.project.update(message, context);
    let Some(snapshot) = action.apply_snapshot else {
        return false;
    };
    state.transport.bpm = snapshot.bpm;
    true
}

#[test]
fn multi_step_redo_retains_future_history_and_supports_another_undo_round_trip() {
    let mut state = AppState::default();
    let initial = state.transport.bpm;
    for bpm in [130.0, 140.0, 150.0] {
        edit_tempo(&mut state, bpm);
    }
    for expected in [140.0, 130.0, initial] {
        assert!(apply_history(&mut state, ProjectMsg::Undo));
        assert_eq!(state.transport.bpm, expected);
    }
    for expected in [130.0, 140.0, 150.0] {
        assert!(
            apply_history(&mut state, ProjectMsg::Redo),
            "missing redo to {expected}"
        );
        assert_eq!(state.transport.bpm, expected);
    }
    assert!(!apply_history(&mut state, ProjectMsg::Redo));
    for expected in [140.0, 130.0, initial] {
        assert!(apply_history(&mut state, ProjectMsg::Undo));
        assert_eq!(state.transport.bpm, expected);
    }
}

#[test]
fn a_new_edit_after_undo_discards_redo_future() {
    let mut state = AppState::default();
    for bpm in [130.0, 140.0, 150.0] {
        edit_tempo(&mut state, bpm);
    }
    assert!(apply_history(&mut state, ProjectMsg::Undo));
    assert!(apply_history(&mut state, ProjectMsg::Undo));
    assert_eq!(state.project.history.redo.len(), 2);
    edit_tempo(&mut state, 175.0);
    assert!(!apply_history(&mut state, ProjectMsg::Redo));
    assert_eq!(state.transport.bpm, 175.0);
    assert!(apply_history(&mut state, ProjectMsg::Undo));
    assert_eq!(state.transport.bpm, 130.0);
}
