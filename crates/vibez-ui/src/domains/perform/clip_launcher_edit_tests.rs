//! Launcher cells can be selected empty, and clips copied, pasted and moved
//! between them.

use super::*;
use crate::domains::perform::PerformMsg;
use crate::domains::test_support::RecordingEngine;
use crate::state::ProjectTrack;

fn tracks(count: usize) -> Vec<ProjectTrack> {
    (0..count)
        .map(|index| {
            let mut track = ProjectTrack::new(TrackId::new(), format!("Part {index}"), index as u8);
            track.kind = vibez_core::midi::TrackKind::Midi;
            track
        })
        .collect()
}

fn launcher() -> PerformState {
    PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    }
}

fn ctx(tracks: &[ProjectTrack]) -> PerformCtx<'_> {
    PerformCtx {
        workspace_visible: true,
        project_tracks: tracks,
        ..Default::default()
    }
}

#[test]
fn clicking_an_empty_cell_selects_it_without_stopping_its_track() {
    let tracks = tracks(1);
    let track_id = tracks[0].id;
    let mut state = launcher();
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::Clips(ClipMsg::CreateMidi { track_id, row: 0 }),
        &mut engine,
        ctx(&tracks),
    );
    assert_eq!(state.clip_editor.slot, Some((track_id, 0)));

    let action = state.update(
        PerformMsg::Clips(ClipMsg::SelectSlot { track_id, row: 3 }),
        &mut engine,
        ctx(&tracks),
    );
    assert_eq!(action.clip_launch, None);
    assert_eq!(action.select_project_track, Some(track_id));
    assert_eq!(state.clip_editor.slot, Some((track_id, 3)));
    assert!(state.clip_editor.selected.is_none());
    assert!(state.timeline_editor().timeline.by_track.is_empty());

    state.update(
        PerformMsg::Clips(ClipMsg::SelectSlot { track_id, row: 0 }),
        &mut engine,
        ctx(&tracks),
    );
    assert_eq!(
        state.clip_editor.selected,
        Some(state.clips.at(track_id, 0).unwrap().id),
        "a cell holding a clip selects that clip"
    );
}
