use super::*;

#[test]
fn add_track_selects_it_and_names_uniquely() {
    let a = arrangement_with_tracks(2);
    assert_eq!(a.tracks.len(), 2);
    assert_eq!(a.tracks[1].name, "Track 2");
    assert_eq!(a.selected_track, Some(a.tracks[1].id));
}

#[test]
fn track_removal_requires_confirmation_then_clears_its_state() {
    let mut a = arrangement_with_tracks(2);
    let victim = a.tracks[1].id;
    let survivor = a.tracks[0].id;
    a.selected_note_clip = Some((victim, ClipId::new()));
    let mut engine = RecordingEngine::default();
    let request = a.update(
        ArrangementMsg::RequestRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 2);
    assert_eq!(a.pending_project_track_deletion, Some(victim));
    assert_eq!(request.close_track_guis, None);
    let action = a.update(
        ArrangementMsg::ConfirmRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.selected_track, Some(survivor));
    assert_eq!(a.selected_note_clip, None);
    assert_eq!(action.close_track_guis, Some(victim));
    assert!(a.arrangement.timeline.get(victim).is_none());
}

#[test]
fn cancelling_track_removal_preserves_the_project_track() {
    let mut a = arrangement_with_tracks(1);
    let victim = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::RequestRemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::CancelRemoveTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.pending_project_track_deletion, None);
    assert!(engine.0.is_empty());
    assert!(!ArrangementMsg::RequestRemoveTrack(victim).marks_dirty());
    assert!(!ArrangementMsg::CancelRemoveTrack.marks_dirty());
    assert!(ArrangementMsg::ConfirmRemoveTrack(victim).marks_dirty());
}

#[test]
fn immediate_track_removal_reuses_the_confirmed_deletion_operation() {
    let mut a = arrangement_with_tracks(2);
    let victim = a.tracks[1].id;
    let mut engine = RecordingEngine::default();

    let action = a.update(
        ArrangementMsg::RemoveTrack(victim),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(a.tracks.len(), 1);
    assert!(a.arrangement.timeline.get(victim).is_none());
    assert_eq!(action.close_track_guis, Some(victim));
    assert_eq!(action.remove_track_from_sections, Some(victim));
    assert!(ArrangementMsg::RemoveTrack(victim).marks_dirty());
}

#[test]
fn removing_a_resample_source_resets_dependent_audio_track_routes() {
    let mut a = arrangement_with_tracks(2);
    let source = a.tracks[0].id;
    let target = a.tracks[1].id;
    a.tracks[1].audio_input_route = AudioInputRoute::Resample { track_id: source };
    let mut engine = RecordingEngine::default();

    a.update(
        ArrangementMsg::RemoveTrack(source),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert_eq!(
        a.tracks
            .iter()
            .find(|track| track.id == target)
            .unwrap()
            .audio_input_route,
        AudioInputRoute::default(),
    );
}

#[test]
fn remove_bus_clears_sends_and_their_automation_lanes() {
    let mut a = arrangement_with_tracks(1);
    let track_id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let bus_id = a.buses[0].id;
    a.tracks[0].sends.push((bus_id, 0.5));
    a.tracks[0]
        .automation
        .push(AutomationLane::new(AutomationTarget::Send { bus_id }));

    a.update(
        ArrangementMsg::RemoveBus(bus_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    let track = a.tracks.iter().find(|track| track.id == track_id).unwrap();
    assert!(track.sends.iter().all(|(id, _)| *id != bus_id));
    assert!(track
        .automation
        .iter()
        .all(|lane| lane.target != AutomationTarget::Send { bus_id }));
}

#[test]
fn reorder_sends_full_order_and_respects_bounds() {
    let mut a = arrangement_with_tracks(2);
    let first = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    // Top track cannot move further up: no command.
    a.update(
        ArrangementMsg::MoveTrackUp(first),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(engine.0.is_empty());
    a.update(
        ArrangementMsg::MoveTrackDown(first),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[1].id, first);
    assert!(matches!(engine.0[0], EngineCommand::ReorderTracks(_)));
}

#[test]
fn gain_and_pan_clamp() {
    let mut a = arrangement_with_tracks(1);
    let id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::SetTrackGain(id, 99.0),
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::SetTrackPan(id, -5.0),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert_eq!(a.tracks[0].gain, 2.0);
    assert_eq!(a.tracks[0].pan, 0.0);
}

#[test]
fn renames_audio_midi_and_bus_channels() {
    let mut a = arrangement_with_tracks(1);
    let audio_id = a.tracks[0].id;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddMidiTrack,
        &mut engine,
        ArrangementCtx::default(),
    );
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let midi_id = a.tracks[1].id;
    let bus_id = a.buses[0].id;

    for (id, name) in [
        (audio_id, "Vocals"),
        (midi_id, "Keys"),
        (bus_id, "Long Reverb"),
    ] {
        a.update(
            ArrangementMsg::RenameTrack(id, name.to_string()),
            &mut engine,
            ArrangementCtx::default(),
        );
    }

    assert_eq!(a.find_track(audio_id).unwrap().name, "Vocals");
    assert_eq!(a.find_track(midi_id).unwrap().name, "Keys");
    assert_eq!(a.find_track(bus_id).unwrap().name, "Long Reverb");
}

#[test]
fn bus_solo_toggles_state_and_sends_the_engine_command() {
    let mut a = arrangement_with_tracks(1);
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::AddBus,
        &mut engine,
        ArrangementCtx::default(),
    );
    let bus_id = a.buses[0].id;
    engine.0.clear();

    a.update(
        ArrangementMsg::SetTrackSolo(bus_id),
        &mut engine,
        ArrangementCtx::default(),
    );

    assert!(a.buses[0].solo);
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetTrackSolo(id, true)] if *id == bus_id
    ));
}

#[test]
fn meter_decays_instead_of_snapping() {
    let mut a = arrangement_with_tracks(1);
    let id = a.tracks[0].id;
    a.tracks[0].peak_l = 1.0;
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::EngineTrackMeter {
            track_id: id,
            peak_l: 0.0,
            peak_r: 0.0,
        },
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!((a.tracks[0].peak_l - 0.85).abs() < 1e-6);
}
