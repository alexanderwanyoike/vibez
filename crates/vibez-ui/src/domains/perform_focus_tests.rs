use super::super::test_support::RecordingEngine;
use super::*;

#[test]
fn window_focus_loss_releases_all_held_instrument_notes_and_clears_overlay() {
    let tracks = vec![playable_midi_track("Drums")];
    let mut state = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(tracks[0].id),
    };
    let at = Instant::now();

    for (key, key_id) in [(ComputerKey::Z, "z"), (ComputerKey::X, "x")] {
        state.update(
            PerformMsg::ComputerKeyPressed {
                key,
                key_id: key_id.into(),
                occurred_at: at,
            },
            &mut engine,
            ctx,
        );
    }
    state.update(
        PerformMsg::SetInstrumentTargetOverlay(true),
        &mut engine,
        ctx,
    );

    state.update(PerformMsg::WindowUnfocused, &mut engine, ctx);

    assert!(!state.is_pad_pressed(PadPosition { row: 3, column: 0 }));
    assert!(!state.is_pad_pressed(PadPosition { row: 3, column: 1 }));
    assert!(!state.instrument_target_overlay);
    assert_eq!(engine.0.len(), 4);
    let mut note_offs = engine
        .0
        .iter()
        .filter_map(|command| match command {
            vibez_engine::commands::EngineCommand::ExternalNoteOff { track_id, pitch } => {
                Some((*track_id, *pitch))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    note_offs.sort_by_key(|(_, pitch)| *pitch);
    assert_eq!(note_offs, [(tracks[0].id, 36), (tracks[0].id, 37)]);
}

fn playable_midi_track(name: &str) -> ProjectTrack {
    let mut track = ProjectTrack::new(TrackId::new(), name.into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    track.has_instrument = true;
    track
}
