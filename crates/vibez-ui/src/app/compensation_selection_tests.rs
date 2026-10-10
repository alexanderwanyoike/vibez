//! Selection changes affect timing only when they change an actual live route.

use super::*;
use crate::domains::{arrangement::ArrangementMsg, perform::PerformMode};
use crate::state::{ProjectTrack, Workspace};
use vibez_core::{id::TrackId, midi::TrackKind};

fn tracks(app: &mut App) -> [TrackId; 2] {
    let ids = [TrackId::new(), TrackId::new()];
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.reduced_latency_monitoring = true;
    for id in ids {
        let mut track = ProjectTrack::new_instrument(id, "Instrument".into(), TrackKind::Midi, 0);
        track.has_instrument = true;
        track.instrument_latency_samples = Some(0);
        project.tracks.push(track);
    }
    ids
}

#[test]
fn unmonitored_selection_does_not_publish_another_timing_plan() {
    let mut app = super::test_support::app();
    let ids = tracks(&mut app);
    let (commands, mut received) = rtrb::RingBuffer::new(16);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.sync_sidechain_routing();
    while received.pop().is_ok() {}
    let generation = app.state.devices.compensation_generation;
    for id in ids {
        let _ = app.update_and_refresh_clips(Message::Arrangement(ArrangementMsg::SelectTrack(id)));
        assert_eq!(app.state.devices.compensation_generation, generation);
        assert!(app
            .state
            .devices
            .last_timing
            .as_ref()
            .unwrap()
            .reduced_tracks
            .is_empty());
        while let Ok(command) = received.pop() {
            assert!(!matches!(
                command,
                EngineCommand::SetRouting(_) | EngineCommand::UpdateAutomationRouting(_)
            ));
        }
    }
}

#[test]
fn unmonitored_selection_does_not_invalidate_the_canonical_routing_cache() {
    let mut app = super::test_support::app();
    let ids = tracks(&mut app);
    let inputs = super::sidechain_sync::RoutingInputs::capture(&app.state, false);
    app.state.arrangement.selected_track = Some(ids[1]);
    app.state
        .perform
        .sync_instrument_target_from_selection(Some(ids[1]), &app.state.project_tracks.tracks);
    assert!(inputs.matches(&app.state, false));
}

#[test]
fn remembered_keyboard_target_is_eligible_only_for_its_configured_perform_route() {
    let mut app = super::test_support::app();
    let ids = tracks(&mut app);
    app.state
        .perform
        .sync_instrument_target_from_selection(Some(ids[0]), &app.state.project_tracks.tracks);
    for (workspace, mode, active) in [
        (Workspace::Arrange, PerformMode::Instrument, false),
        (Workspace::Mix, PerformMode::Instrument, false),
        (Workspace::Perform, PerformMode::Sections, false),
        (Workspace::Perform, PerformMode::TrackMutes, false),
        (Workspace::Perform, PerformMode::Instrument, true),
    ] {
        app.state.view.workspace = workspace;
        app.state.perform.mode = mode;
        assert_eq!(
            app.compensation_signature_for_input(false).reduced_tracks,
            if active { vec![ids[0]] } else { vec![] },
            "{workspace:?}/{mode:?}"
        );
    }
}

#[test]
fn connected_midi_follows_the_same_destination_as_the_actual_adapter() {
    let mut app = super::test_support::app();
    let ids = tracks(&mut app);
    for id in ids {
        app.state.arrangement.selected_track = Some(id);
        assert_eq!(super::midi_input::configured_target(&app.state), Some(id));
        assert_eq!(
            app.compensation_signature_for_input(true).reduced_tracks,
            vec![id]
        );
        assert!(app
            .compensation_signature_for_input(false)
            .reduced_tracks
            .is_empty());
    }
    let inputs = super::sidechain_sync::RoutingInputs::capture(&app.state, true);
    app.state.arrangement.selected_track = Some(ids[0]);
    assert!(!inputs.matches(&app.state, true));
}

#[test]
fn full_compensation_does_not_prepare_again_for_input_target_or_mode_changes() {
    let mut app = super::test_support::app();
    let ids = tracks(&mut app);
    Arc::make_mut(&mut app.state.project_tracks).reduced_latency_monitoring = false;
    let inputs = super::sidechain_sync::RoutingInputs::capture(&app.state, false);
    app.state.arrangement.selected_track = Some(ids[0]);
    app.state.view.workspace = Workspace::Perform;
    app.state.perform.mode = PerformMode::Instrument;
    app.state
        .perform
        .sync_instrument_target_from_selection(Some(ids[0]), &app.state.project_tracks.tracks);
    assert!(inputs.matches(&app.state, true));
    assert!(app
        .compensation_signature_for_input(true)
        .reduced_tracks
        .is_empty());
}
