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

fn create(
    state: &mut PerformState,
    tracks: &[ProjectTrack],
    track_id: TrackId,
    row: u32,
) -> ClipId {
    state.update(
        PerformMsg::Clips(ClipMsg::CreateMidi { track_id, row }),
        &mut RecordingEngine::default(),
        ctx(tracks),
    );
    state.clip_editor.selected.unwrap()
}

fn send(state: &mut PerformState, tracks: &[ProjectTrack], msg: ClipMsg) -> PerformAction {
    state.update(
        PerformMsg::Clips(msg),
        &mut RecordingEngine::default(),
        ctx(tracks),
    )
}

fn content_mut(state: &mut PerformState, id: ClipId) -> &mut crate::state::TrackTimelineContent {
    let clip = Arc::make_mut(&mut state.clips).by_id_mut(id).unwrap();
    Arc::make_mut(&mut clip.timeline).ensure(clip.track_id)
}

fn lane_targets(clip: &LauncherClip) -> Vec<vibez_core::automation::AutomationTarget> {
    clip.timeline
        .get(clip.track_id)
        .unwrap()
        .automation
        .iter()
        .map(|lane| lane.target)
        .collect()
}

#[test]
fn paste_puts_an_independent_copy_in_the_selected_cell_and_replaces_what_was_there() {
    let tracks = tracks(1);
    let track = tracks[0].id;
    let mut state = launcher();
    let original = create(&mut state, &tracks, track, 0);
    content_mut(&mut state, original).note_clips[0].name = "Groove".into();

    send(&mut state, &tracks, ClipMsg::Copy);
    send(
        &mut state,
        &tracks,
        ClipMsg::SelectSlot {
            track_id: track,
            row: 2,
        },
    );
    let action = send(&mut state, &tracks, ClipMsg::Paste);
    let pasted = state.clips.at(track, 2).unwrap().clone();
    assert_ne!(pasted.id, original);
    assert_eq!(pasted.name(), "Groove");
    assert_eq!(
        pasted.timeline.get(track).unwrap().note_clips[0].id,
        pasted.id
    );
    assert_eq!(state.clip_editor.selected, Some(pasted.id));
    assert_eq!(action.select_project_track, Some(track));
    assert!(state.clips.by_id(original).is_some());

    send(&mut state, &tracks, ClipMsg::Paste);
    assert_eq!(
        state.clips.clips.len(),
        2,
        "a paste onto a clip replaces it"
    );
    assert_ne!(state.clips.at(track, 2).unwrap().id, pasted.id);
}

#[test]
fn cut_stops_a_playing_clip_and_paste_puts_it_back_elsewhere() {
    let tracks = tracks(1);
    let track = tracks[0].id;
    let mut state = launcher();
    let id = create(&mut state, &tracks, track, 0);
    let clip = state.clips.by_id(id).unwrap().clone();
    state.clip_editor.playing.insert(track, clip);

    let action = send(&mut state, &tracks, ClipMsg::Cut);
    assert_eq!(action.clip_launch, Some(ClipLaunchRequest::Stop(track)));
    assert!(state.clips.at(track, 0).is_none());

    send(
        &mut state,
        &tracks,
        ClipMsg::SelectSlot {
            track_id: track,
            row: 1,
        },
    );
    send(&mut state, &tracks, ClipMsg::Paste);
    assert!(state.clips.at(track, 1).is_some());
}

#[test]
fn paste_onto_another_track_keeps_track_lanes_and_drops_device_lanes() {
    use vibez_core::automation::{AutomationLane, AutomationTarget};
    let tracks = tracks(2);
    let (from, to) = (tracks[0].id, tracks[1].id);
    let mut state = launcher();
    let id = create(&mut state, &tracks, from, 0);
    let lanes = &mut content_mut(&mut state, id).automation;
    lanes.push(AutomationLane::new(AutomationTarget::TrackGain));
    lanes.push(AutomationLane::new(AutomationTarget::InstrumentParam {
        param_index: 0,
    }));

    send(&mut state, &tracks, ClipMsg::Copy);
    send(
        &mut state,
        &tracks,
        ClipMsg::SelectSlot {
            track_id: to,
            row: 0,
        },
    );
    send(&mut state, &tracks, ClipMsg::Paste);
    let pasted = state.clips.at(to, 0).unwrap();
    assert_eq!(pasted.track_id, to);
    assert!(pasted.timeline.get(from).is_none());
    assert_eq!(
        lane_targets(pasted),
        vec![AutomationTarget::TrackGain],
        "another track's devices are not this clip's"
    );
    assert_eq!(lane_targets(state.clips.by_id(id).unwrap()).len(), 2);
}

#[test]
fn paste_refuses_a_cell_on_a_track_of_the_other_kind() {
    let mut tracks = tracks(2);
    tracks[1].kind = vibez_core::midi::TrackKind::Audio;
    let (midi, audio) = (tracks[0].id, tracks[1].id);
    let mut state = launcher();
    create(&mut state, &tracks, midi, 0);

    send(&mut state, &tracks, ClipMsg::Copy);
    send(
        &mut state,
        &tracks,
        ClipMsg::SelectSlot {
            track_id: audio,
            row: 0,
        },
    );
    let action = send(&mut state, &tracks, ClipMsg::Paste);
    assert!(state.clips.at(audio, 0).is_none());
    assert!(action.status.is_some());
}

#[test]
fn dropping_a_clip_moves_it_and_a_command_drop_copies_it() {
    let tracks = tracks(2);
    let (a, b) = (tracks[0].id, tracks[1].id);
    let mut state = launcher();
    let id = create(&mut state, &tracks, a, 0);
    let drop = |track_id, row, copy| ClipMsg::Drop {
        clip: id,
        track_id,
        row,
        copy,
    };

    send(&mut state, &tracks, ClipMsg::BeginDrag(id));
    assert_eq!(state.clip_editor.dragging, Some(id));
    send(&mut state, &tracks, drop(a, 3, false));
    assert_eq!(state.clip_editor.dragging, None);
    assert_eq!(
        state.clips.at(a, 3).map(|clip| clip.id),
        Some(id),
        "a move along its own track keeps the clip, so it keeps playing"
    );
    assert!(state.clips.at(a, 0).is_none());
    assert_eq!(state.clip_editor.slot, Some((a, 3)));

    send(&mut state, &tracks, drop(a, 1, true));
    assert_eq!(state.clips.at(a, 3).map(|clip| clip.id), Some(id));
    assert!(state.clips.at(a, 1).is_some_and(|clip| clip.id != id));

    send(&mut state, &tracks, drop(b, 0, false));
    assert!(state.clips.by_id(id).is_none());
    let moved = state.clips.at(b, 0).unwrap();
    assert!(moved.timeline.get(b).is_some());
    assert_eq!(state.clip_editor.selected, Some(moved.id));
}

#[test]
fn a_drop_onto_a_track_of_the_other_kind_leaves_the_clip_where_it_was() {
    let mut tracks = tracks(2);
    tracks[1].kind = vibez_core::midi::TrackKind::Audio;
    let (midi, audio) = (tracks[0].id, tracks[1].id);
    let mut state = launcher();
    let id = create(&mut state, &tracks, midi, 0);

    send(&mut state, &tracks, ClipMsg::BeginDrag(id));
    let action = send(
        &mut state,
        &tracks,
        ClipMsg::Drop {
            clip: id,
            track_id: audio,
            row: 0,
            copy: false,
        },
    );
    assert!(action.status.is_some());
    assert_eq!(state.clips.at(midi, 0).map(|clip| clip.id), Some(id));
    assert_eq!(state.clip_editor.dragging, None);
}
