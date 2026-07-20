use super::super::test_support::RecordingEngine;
use super::*;

fn playable_track() -> ProjectTrack {
    let mut track = ProjectTrack::new(TrackId::new(), "Hats".into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    track.has_instrument = true;
    track
}

fn instrument_state() -> PerformState {
    PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    }
}

#[test]
fn momentary_repeat_starts_with_resolved_note_and_pad_release_stops_it() {
    let tracks = vec![playable_track()];
    let mut state = instrument_state();
    state.set_fixed_computer_velocity(113);
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(tracks[0].id),
    };
    let at = Instant::now();

    let action = state.update(PerformMsg::SetNoteRepeatMomentary(true), &mut engine, ctx);
    assert!(action.keyboard_consumed);
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::SetNoteRepeatRate(NoteRepeatRate::ThirtySecondTriplet),
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );

    assert!(matches!(
        engine.0.as_slice(),
        [
            vibez_engine::commands::EngineCommand::ExternalNoteOn {
                track_id,
                pitch: 36,
                velocity: 113,
            },
            vibez_engine::commands::EngineCommand::StartNoteRepeat {
                id: 12,
                track_id: repeat_track,
                pitch: 36,
                velocity: 113,
                rate: NoteRepeatRate::Sixteenth,
            },
            vibez_engine::commands::EngineCommand::UpdateNoteRepeatRate {
                id: 12,
                track_id: update_track,
                rate: NoteRepeatRate::ThirtySecondTriplet,
            },
            vibez_engine::commands::EngineCommand::StopNoteRepeat {
                id: 12,
                track_id: stop_track,
            },
            vibez_engine::commands::EngineCommand::ExternalNoteOff {
                track_id: off_track,
                pitch: 36,
            },
        ] if *track_id == tracks[0].id
            && *repeat_track == tracks[0].id
            && *update_track == tracks[0].id
            && *stop_track == tracks[0].id
            && *off_track == tracks[0].id
    ));
}

#[test]
fn onscreen_latch_can_start_held_pad_and_unlatch_without_retriggering() {
    let tracks = vec![playable_track()];
    let mut state = instrument_state();
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(tracks[0].id),
    };
    let at = Instant::now();

    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::ToggleNoteRepeatLatch, &mut engine, ctx);
    state.update(PerformMsg::ToggleNoteRepeatLatch, &mut engine, ctx);
    state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );

    assert!(matches!(
        engine.0.as_slice(),
        [
            vibez_engine::commands::EngineCommand::ExternalNoteOn { .. },
            vibez_engine::commands::EngineCommand::StartNoteRepeat { .. },
            vibez_engine::commands::EngineCommand::StopNoteRepeat { .. },
            vibez_engine::commands::EngineCommand::ExternalNoteOff { .. },
        ]
    ));
    assert!(!state.note_repeat_latched());
}

#[test]
fn swing_edits_are_project_dirty_engine_commands_with_track_inheritance() {
    let tracks = vec![playable_track()];
    let mut state = instrument_state();
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(tracks[0].id),
    };

    state.update(PerformMsg::SetProjectSwing(0.4), &mut engine, ctx);
    let action = state.update(
        PerformMsg::SetTrackSwingOffset(Some(-0.1)),
        &mut engine,
        ctx,
    );
    assert_eq!(state.project_swing(), SwingAmount::new(0.4));
    assert_eq!(
        action.track_swing_request,
        Some(TrackSwingRequest {
            track_id: tracks[0].id,
            swing_offset: Some(SwingOffset::new(-0.1)),
        })
    );
    assert!(PerformMsg::SetProjectSwing(0.4).marks_dirty());
    assert!(PerformMsg::SetTrackSwingOffset(None).marks_dirty());
    assert!(matches!(
        engine.0.as_slice(),
        [
            vibez_engine::commands::EngineCommand::SetProjectSwing(amount),
            vibez_engine::commands::EngineCommand::SetTrackSwingOffset(
                track_id,
                Some(offset),
            ),
        ] if *amount == SwingAmount::new(0.4)
            && *track_id == tracks[0].id
            && *offset == SwingOffset::new(-0.1)
    ));
}
