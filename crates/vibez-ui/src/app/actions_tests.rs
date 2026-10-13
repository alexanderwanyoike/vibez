use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::{AppState, ProjectSnapshot, ProjectTrack};
use vibez_core::id::TrackId;

fn snapshot(state: &AppState) -> ProjectSnapshot {
    state.project_snapshot()
}

#[test]
fn perform_mute_request_updates_the_shared_track_and_engine_together() {
    let track_id = TrackId::new();
    let mut state = AppState::default();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Drums".into(), 0));
    let mut engine = RecordingEngine::default();
    let pre_edit_snapshot = snapshot(&state);

    let name = apply_track_mute_request(
        &mut state.project_tracks,
        &mut state.project.history,
        pre_edit_snapshot,
        crate::domains::perform::TrackMuteRequest {
            track_id,
            muted: true,
            quantization: vibez_core::perform::TrackMuteQuantization::Immediate,
        },
        false,
        &mut engine,
    );

    assert_eq!(name.as_deref(), Some("Drums"));
    assert!(state.project_tracks.tracks[0].mute);
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetTrackMute(event_track, true)] if *event_track == track_id
    ));
    assert_eq!(state.project.history.undo.len(), 1);
    let before_mute = state.project.history.pop_undo().expect("mute undo step");
    assert!(!before_mute.project_tracks.tracks[0].mute);
}

#[test]
fn missing_track_mute_request_does_not_create_an_undo_step() {
    let mut state = AppState::default();
    let mut engine = RecordingEngine::default();
    let pre_edit_snapshot = snapshot(&state);

    let name = apply_track_mute_request(
        &mut state.project_tracks,
        &mut state.project.history,
        pre_edit_snapshot,
        crate::domains::perform::TrackMuteRequest {
            track_id: TrackId::new(),
            muted: true,
            quantization: vibez_core::perform::TrackMuteQuantization::Immediate,
        },
        false,
        &mut engine,
    );

    assert_eq!(name, None);
    assert!(state.project.history.undo.is_empty());
    assert!(engine.0.is_empty());
}

#[test]
fn running_quantized_mute_waits_for_engine_truth_before_editing_project_state() {
    let track_id = TrackId::new();
    let mut state = AppState::default();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Bass".into(), 0));
    let mut engine = RecordingEngine::default();
    let pre_edit_snapshot = snapshot(&state);

    let name = apply_track_mute_request(
        &mut state.project_tracks,
        &mut state.project.history,
        pre_edit_snapshot,
        crate::domains::perform::TrackMuteRequest {
            track_id,
            muted: true,
            quantization: vibez_core::perform::TrackMuteQuantization::OneBar,
        },
        true,
        &mut engine,
    );

    assert_eq!(name.as_deref(), Some("Bass"));
    assert!(!state.project_tracks.tracks[0].mute);
    assert!(state.project.history.undo.is_empty());
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::QueueTrackMute {
            track_id: event_track,
            muted: true,
            quantization: vibez_core::perform::TrackMuteQuantization::OneBar,
        }] if *event_track == track_id
    ));
}

#[test]
fn section_refresh_is_prepared_only_for_the_currently_playing_section() {
    let mut state = AppState::default();
    let playing = crate::domains::perform::Section::new(0);
    let playing_id = playing.id;
    let other = crate::domains::perform::Section::new(1);
    let other_id = other.id;
    Arc::make_mut(&mut state.perform.sections).insert(playing);
    Arc::make_mut(&mut state.perform.sections).insert(other);
    state.perform.playing_section = Some(playing_id);

    assert!(prepare_playing_section_refresh(
        &state.perform,
        &state.project_tracks.tracks,
        playing_id,
    )
    .is_some());
    assert!(prepare_playing_section_refresh(
        &state.perform,
        &state.project_tracks.tracks,
        other_id,
    )
    .is_none());
}

#[test]
fn section_slice_refresh_is_requested_only_after_a_new_project_track_is_replayed() {
    let section_id = crate::domains::perform::Section::new(0).id;
    let track_id = TrackId::new();

    assert_eq!(
        section_to_refresh_after_project_track_replay(
            Some(track_id),
            vibez_project::TimelineLocation::Section(section_id),
        ),
        Some(section_id)
    );
    assert_eq!(
        section_to_refresh_after_project_track_replay(
            None,
            vibez_project::TimelineLocation::Section(section_id),
        ),
        None
    );
    assert_eq!(
        section_to_refresh_after_project_track_replay(
            Some(track_id),
            vibez_project::TimelineLocation::Arrange,
        ),
        None
    );
}
