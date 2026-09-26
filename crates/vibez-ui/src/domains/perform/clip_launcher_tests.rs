//! Clip authoring preserves independent content, selection and controller navigation.

use super::*;
use crate::domains::perform::{PerformMode, PerformMsg};
use crate::domains::piano_roll::{PianoRollCtx, PianoRollMsg};
use crate::domains::test_support::RecordingEngine;
use crate::state::{AppState, ProjectTrack, Workspace};

fn tracks(count: usize) -> Vec<ProjectTrack> {
    (0..count)
        .map(|index| {
            let mut track = ProjectTrack::new(TrackId::new(), format!("Part {index}"), index as u8);
            track.kind = vibez_core::midi::TrackKind::Midi;
            track
        })
        .collect()
}

#[test]
fn clip_projects_reuse_the_note_editor_without_changing_arrange_or_sections() {
    let tracks = tracks(1);
    let track_id = tracks[0].id;
    let mut state = AppState::default();
    state.view.workspace = Workspace::Perform;
    state.perform.layout = PerformLayout::Clips;
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(track_id),
    };
    state.perform.update(
        PerformMsg::Clips(ClipMsg::CreateMidi { track_id, row: 12 }),
        &mut engine,
        ctx,
    );
    let id = state.perform.clip_editor.selected.unwrap();
    let before = state.project_snapshot();
    state.piano_roll.update(
        PianoRollMsg::AddNote {
            track_id,
            clip_id: id,
            pitch: 42,
            start_beat: 0.5,
            duration_beats: 0.25,
        },
        &mut crate::domains::DiscardingEngine,
        state.perform.timeline_editor_mut(),
        PianoRollCtx::default(),
    );
    state.perform.commit_selected_timeline();
    assert_eq!(
        state
            .perform
            .clips
            .by_id(id)
            .unwrap()
            .timeline
            .get(track_id)
            .unwrap()
            .note_clips[0]
            .notes
            .len(),
        1
    );
    assert!(before
        .launcher_clips
        .by_id(id)
        .unwrap()
        .timeline
        .get(track_id)
        .unwrap()
        .note_clips[0]
        .notes
        .is_empty());
    assert!(state.arrangement.timeline.by_track.is_empty());
    assert!(state.perform.sections.sections.is_empty());
    assert!(engine.0.is_empty());
    assert_eq!(
        state.active_timeline_editor().selected_note_clip,
        Some((track_id, id))
    );
}

#[test]
fn window_moves_beyond_four_tracks_and_survives_mode_changes() {
    let tracks = tracks(11);
    let mut state = PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        ..Default::default()
    };
    state.update(
        PerformMsg::Clips(ClipMsg::MoveWindow { tracks: 4, rows: 8 }),
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::SelectMode(PerformMode::Instrument),
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::SelectMode(PerformMode::Sections),
        &mut engine,
        ctx,
    );
    assert_eq!(
        (state.clip_editor.first_track, state.clip_editor.first_row),
        (4, 8)
    );
    state.update(
        PerformMsg::Clips(ClipMsg::MoveWindow {
            tracks: 99,
            rows: -99,
        }),
        &mut engine,
        ctx,
    );
    assert_eq!(
        (state.clip_editor.first_track, state.clip_editor.first_row),
        (7, 0)
    );
    assert!(engine.0.is_empty());
}

#[test]
fn section_projects_reject_clip_authoring_and_clip_projects_reject_section_creation() {
    let tracks = tracks(1);
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        ..Default::default()
    };
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::Clips(ClipMsg::CreateMidi {
            track_id: tracks[0].id,
            row: 0,
        }),
        &mut engine,
        ctx,
    );
    assert!(state.clips.clips.is_empty());
    state.layout = PerformLayout::Clips;
    state.update(PerformMsg::CreateSectionAt(0), &mut engine, ctx);
    assert!(state.sections.sections.is_empty());
}

#[test]
fn empty_clip_project_never_exposes_hidden_arrange_selection() {
    let mut state = AppState::default();
    state.view.workspace = Workspace::Perform;
    state.perform.layout = PerformLayout::Clips;
    state.arrangement.selected_note_clip = Some((TrackId::new(), ClipId::new()));
    assert!(state.active_timeline_editor().selected_note_clip.is_none());
}

#[test]
fn duplicate_creates_an_independent_alternative_and_editor_delete_frees_the_slot() {
    let tracks = tracks(1);
    let track_id = tracks[0].id;
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        ..Default::default()
    };
    let mut state = PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::Clips(ClipMsg::CreateMidi { track_id, row: 0 }),
        &mut engine,
        ctx,
    );
    let original = state.clip_editor.selected.unwrap();
    state.update(
        PerformMsg::Clips(ClipMsg::Duplicate(original)),
        &mut engine,
        ctx,
    );
    let duplicate = state.clip_editor.selected.unwrap();
    assert_ne!(original, duplicate);
    assert_eq!(state.clips.at(track_id, 1).unwrap().id, duplicate);
    assert_eq!(
        state
            .timeline_editor()
            .timeline
            .get(track_id)
            .unwrap()
            .note_clips[0]
            .id,
        duplicate
    );
    Arc::make_mut(&mut state.timeline_editor_mut().timeline)
        .ensure(track_id)
        .note_clips
        .clear();
    state.commit_selected_timeline();
    assert!(state.clips.at(track_id, 1).is_none());
    assert!(state.clips.at(track_id, 0).is_some());
    assert!(state.clip_editor.selected.is_none());
    assert!(engine.0.is_empty());
}

#[test]
fn keyboard_window_launches_later_tracks_without_changing_the_edit_selection_or_repeating() {
    use super::super::{ComputerKey, PerformMode, PerformMsg};
    let tracks = tracks(8);
    let mut perform = PerformState {
        layout: PerformLayout::Clips,
        mode: PerformMode::Sections,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: None,
    };
    perform.update(
        PerformMsg::Clips(ClipMsg::CreateMidi {
            track_id: tracks[5].id,
            row: 7,
        }),
        &mut engine,
        ctx,
    );
    let launched = perform.clip_editor.selected.unwrap();
    perform.update(
        PerformMsg::Clips(ClipMsg::CreateMidi {
            track_id: tracks[0].id,
            row: 0,
        }),
        &mut engine,
        ctx,
    );
    let edited = perform.clip_editor.selected;
    perform.clip_editor.first_track = 4;
    perform.clip_editor.first_row = 7;
    let key = PerformMsg::ComputerKeyPressed {
        key: ComputerKey::Digit2,
        key_id: "2".into(),
        occurred_at: std::time::Instant::now(),
    };
    assert_eq!(
        perform.update(key.clone(), &mut engine, ctx).clip_launch,
        Some(ClipLaunchRequest::Clip(launched))
    );
    assert_eq!(perform.clip_editor.selected, edited);
    assert_eq!(perform.update(key, &mut engine, ctx).clip_launch, None);
    assert!(engine.0.is_empty());
}

#[test]
fn loop_is_default_and_cell_toggle_updates_the_selected_editor_and_prepared_source() {
    let tracks = tracks(1);
    let track_id = tracks[0].id;
    let mut state = PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    };
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(track_id),
    };
    state.update_clips(ClipMsg::CreateMidi { track_id, row: 0 }, ctx);
    let id = state.clip_editor.selected.unwrap();
    assert!(state.clips.by_id(id).unwrap().prepare(0, 100.0).looping);
    state.update_clips(ClipMsg::ToggleLoop(id), ctx);
    assert!(!state.clips.by_id(id).unwrap().prepare(0, 100.0).looping);
    assert!(
        !state
            .clip_editor
            .editor
            .timeline
            .get(track_id)
            .unwrap()
            .note_clips[0]
            .loop_enabled
    );
    state.update_clips(ClipMsg::ToggleLoop(id), ctx);
    assert!(state.clips.by_id(id).unwrap().prepare(0, 100.0).looping);
    assert!(state.sections.sections.is_empty());
}

#[test]
fn clip_triggers_toggle_playing_and_queued_intent_without_changing_the_editor() {
    let tracks = tracks(1);
    let track = tracks[0].id;
    let mut perform = PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        ..Default::default()
    };
    perform.update(
        PerformMsg::Clips(ClipMsg::CreateMidi {
            track_id: track,
            row: 0,
        }),
        &mut engine,
        ctx,
    );
    let id = perform.clip_editor.selected.unwrap();
    let trigger = PerformMsg::Clips(ClipMsg::Toggle(id));
    assert_eq!(
        perform
            .update(trigger.clone(), &mut engine, ctx)
            .clip_launch,
        Some(ClipLaunchRequest::Clip(id))
    );
    perform.clip_editor.queued.insert(track, Some(id));
    assert_eq!(
        perform
            .update(trigger.clone(), &mut engine, ctx)
            .clip_launch,
        Some(ClipLaunchRequest::Stop(track))
    );
    perform.clip_editor.queued.clear();
    perform
        .clip_editor
        .playing
        .insert(track, perform.clips.by_id(id).unwrap().clone());
    assert_eq!(
        perform
            .update(trigger.clone(), &mut engine, ctx)
            .clip_launch,
        Some(ClipLaunchRequest::Stop(track))
    );
    perform.clip_editor.queued.insert(track, None);
    assert_eq!(
        perform.update(trigger, &mut engine, ctx).clip_launch,
        Some(ClipLaunchRequest::Clip(id))
    );
    assert_eq!(perform.clip_editor.selected, Some(id));
}

#[test]
fn an_empty_grid_key_stops_its_track_instead_of_creating_a_clip() {
    let tracks = tracks(1);
    let mut perform = PerformState {
        layout: PerformLayout::Clips,
        ..Default::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        ..Default::default()
    };
    let action = perform.update(
        PerformMsg::ComputerKeyPressed {
            key: crate::domains::perform::ComputerKey::Digit1,
            key_id: "1".into(),
            occurred_at: std::time::Instant::now(),
        },
        &mut engine,
        ctx,
    );
    assert_eq!(
        action.clip_launch,
        Some(ClipLaunchRequest::Stop(tracks[0].id))
    );
    assert!(perform.clips.clips.is_empty());
}

#[test]
fn stale_engine_acknowledgements_cannot_undo_a_newer_toggle() {
    let track = TrackId::new();
    let clip = vibez_core::id::ClipId::new();
    let mut editor = ClipEditor::default();
    editor.queue_request(track, Some(clip), 1);
    editor.queue_request(track, None, 2);
    editor.queue_request(track, Some(clip), 1);
    editor.acknowledge_transition(track, 1);
    assert_eq!(editor.queued.get(&track), Some(&None));
    editor.queue_request(track, Some(clip), 3);
    editor.queue_request(track, None, 2);
    editor.acknowledge_transition(track, 2);
    assert_eq!(editor.queued.get(&track), Some(&Some(clip)));
    editor.acknowledge_transition(track, 3);
    assert!(!editor.queued.contains_key(&track));
}

#[test]
fn deleting_an_active_or_queued_clip_requests_a_track_stop() {
    for queued in [false, true] {
        let tracks = tracks(1);
        let track = tracks[0].id;
        let mut state = PerformState {
            layout: PerformLayout::Clips,
            ..Default::default()
        };
        let mut engine = RecordingEngine::default();
        let ctx = PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            ..Default::default()
        };
        state.update(
            PerformMsg::Clips(ClipMsg::CreateMidi {
                track_id: track,
                row: 0,
            }),
            &mut engine,
            ctx,
        );
        let clip = state.clips.clips[0].clone();
        if queued {
            state.clip_editor.queue_request(track, Some(clip.id), 1);
        } else {
            state.clip_editor.playing.insert(track, clip.clone());
        }
        let action = state.update(
            PerformMsg::Clips(ClipMsg::Delete(clip.id)),
            &mut engine,
            ctx,
        );
        assert_eq!(action.clip_launch, Some(ClipLaunchRequest::Stop(track)));
        assert!(state.clips.by_id(clip.id).is_none());
    }
}

#[test]
fn clip_playhead_and_progress_share_loop_and_one_shot_boundaries() {
    let track = TrackId::new();
    let mut clip =
        super::super::clip_record::empty_midi_clip(ClipId::new(), track, 0, "Take".into(), 4.0);
    let mut editor = ClipEditor::default();
    editor.started_at.insert(track, 10);
    editor.playing.insert(track, clip.clone());
    assert_eq!(editor.playhead_samples(clip.id, 29, 4.0), Some(3));
    assert_eq!(editor.progress(clip.id, 29, 4.0), Some(3.0 / 16.0));
    assert_eq!(editor.playhead_samples(ClipId::new(), 29, 4.0), None);
    Arc::make_mut(&mut clip.timeline).ensure(track).note_clips[0].loop_enabled = false;
    editor.playing.insert(track, clip.clone());
    assert_eq!(editor.playhead_samples(clip.id, 29, 4.0), Some(16));
    assert_eq!(editor.progress(clip.id, 29, 4.0), Some(1.0));
}
