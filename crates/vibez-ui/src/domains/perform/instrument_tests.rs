use super::super::{ComputerKey, PerformCtx, PerformMsg};
use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::ProjectTrack;

fn position(level: u8) -> PadPosition {
    PadPosition::ALL
        .into_iter()
        .find(|position| position.ordinal(PerformMode::Instrument) == level + 1)
        .expect("level maps to a Pad Position")
}

#[test]
fn descriptors_distribute_values_without_parameter_specific_pad_logic() {
    let pitch = SixteenLevelsParameter::Pitch.descriptor();
    let velocity = SixteenLevelsParameter::Velocity.descriptor();

    let pitch_values = (0..16)
        .map(|level| pitch.default_range.value_at(position(level)))
        .collect::<Vec<_>>();
    assert_eq!(pitch_values, (0..16).collect::<Vec<_>>());

    let velocity_values = (0..16)
        .map(|level| velocity.default_range.value_at(position(level)))
        .collect::<Vec<_>>();
    assert_eq!(velocity_values.first(), Some(&8));
    assert_eq!(velocity_values.last(), Some(&127));
    assert!(velocity_values
        .windows(2)
        .all(|pair| matches!(pair[1] - pair[0], 7 | 8)));

    let input = InstrumentPadPreview {
        pitch: 60,
        velocity: 41,
    };
    assert_eq!(
        (pitch.apply)(input, 7),
        InstrumentPadPreview {
            pitch: 67,
            velocity: 41
        }
    );
    assert_eq!(
        (velocity.apply)(input, 99),
        InstrumentPadPreview {
            pitch: 60,
            velocity: 99
        }
    );
}

#[test]
fn full_level_is_reversible_and_does_not_rewrite_mapping_or_sections() {
    let track_id = TrackId::new();
    let mut state = PerformState::default();
    let mapping = state.input_mapping.clone();
    let sections = std::sync::Arc::clone(&state.sections);

    let normal = state.resolve_instrument_note(position(0), 23, track_id);
    state.toggle_full_level();
    let full = state.resolve_instrument_note(position(1), 23, track_id);
    state.toggle_full_level();
    let restored = state.resolve_instrument_note(position(2), 23, track_id);

    assert_eq!(normal.velocity, 23);
    assert_eq!(full.velocity, 127);
    assert_eq!(restored.velocity, 23);
    assert_eq!(state.input_mapping, mapping);
    assert!(std::sync::Arc::ptr_eq(&state.sections, &sections));
}

#[test]
fn pitch_levels_preserve_velocity_and_coexist_with_full_level() {
    let track_id = TrackId::new();
    let mut state = PerformState::default();
    let source = state.resolve_instrument_note(position(0), 44, track_id);
    assert_eq!(source.pitch, 36);

    state.toggle_sixteen_levels();
    let high = state.resolve_instrument_note(position(15), 57, track_id);
    assert_eq!((high.pitch, high.velocity), (51, 57));

    state.toggle_full_level();
    let middle = state.resolve_instrument_note(position(7), 19, track_id);
    assert_eq!((middle.pitch, middle.velocity), (43, 127));
    assert!(state.full_level_available());
}

#[test]
fn velocity_levels_own_velocity_and_make_full_level_unavailable() {
    let track_id = TrackId::new();
    let mut state = PerformState::default();
    state.resolve_instrument_note(position(4), 31, track_id);
    state.toggle_full_level();
    state.select_sixteen_levels_parameter(SixteenLevelsParameter::Velocity);
    state.toggle_sixteen_levels();

    assert!(state.full_level_enabled());
    assert!(!state.full_level_available());
    assert!(!state.full_level_effective());
    state.toggle_full_level();
    assert!(
        state.full_level_enabled(),
        "disabled control ignores toggles"
    );

    let low = state.resolve_instrument_note(position(0), 2, track_id);
    let high = state.resolve_instrument_note(position(15), 2, track_id);
    assert_eq!((low.pitch, low.velocity), (40, 8));
    assert_eq!((high.pitch, high.velocity), (40, 127));
}

#[test]
fn choose_source_and_target_changes_have_an_explicit_lifecycle() {
    let first_target = TrackId::new();
    let second_target = TrackId::new();
    let mut state = PerformState::default();

    state.sync_instrument_target(Some(first_target));
    state.toggle_sixteen_levels();
    assert!(state.choosing_sixteen_levels_source());
    assert_eq!(state.sixteen_levels_source_pitch(), None);

    let chosen = state.resolve_instrument_note(position(6), 70, first_target);
    assert_eq!(chosen.pitch, 42);
    assert_eq!(state.sixteen_levels_source_pitch(), Some(42));
    assert!(!state.choosing_sixteen_levels_source());

    state.begin_choosing_sixteen_levels_source();
    state.resolve_instrument_note(position(2), 70, first_target);
    assert_eq!(state.sixteen_levels_source_pitch(), Some(38));

    state.sync_instrument_target(Some(second_target));
    assert_eq!(state.sixteen_levels_source_pitch(), None);
    assert!(state.choosing_sixteen_levels_source());
    let newly_chosen = state.resolve_instrument_note(position(10), 70, second_target);
    assert_eq!(newly_chosen.pitch, 46);
    assert_eq!(newly_chosen.track_id, second_target);
}

#[test]
fn selecting_a_non_playable_track_preserves_the_last_instrument_target_and_source() {
    let mut first_instrument = ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
    first_instrument.kind = vibez_core::midi::TrackKind::Midi;
    first_instrument.has_instrument = true;
    let audio_track = ProjectTrack::new(TrackId::new(), "Vocal".into(), 1);
    let mut second_instrument = ProjectTrack::new(TrackId::new(), "Bass".into(), 2);
    second_instrument.kind = vibez_core::midi::TrackKind::Midi;
    second_instrument.has_instrument = true;
    let tracks = vec![first_instrument, audio_track, second_instrument];
    let mut state = PerformState::default();

    state.sync_instrument_target_from_selection(Some(tracks[0].id), &tracks);
    assert_eq!(state.instrument_target(), Some(tracks[0].id));
    state.resolve_instrument_note(position(4), 70, tracks[0].id);
    state.toggle_sixteen_levels();
    assert_eq!(state.sixteen_levels_source_pitch(), Some(40));

    state.sync_instrument_target_from_selection(Some(tracks[1].id), &tracks);
    assert_eq!(state.instrument_target(), Some(tracks[0].id));
    assert_eq!(state.sixteen_levels_source_pitch(), Some(40));
    assert!(!state.choosing_sixteen_levels_source());

    state.mode = PerformMode::Instrument;
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: std::time::Instant::now(),
        },
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: Some(tracks[1].id),
        },
    );
    assert!(matches!(
        engine.0.last(),
        Some(vibez_engine::commands::EngineCommand::ExternalNoteOn { track_id, .. })
            if *track_id == tracks[0].id
    ));

    state.sync_instrument_target_from_selection(Some(tracks[2].id), &tracks);
    assert_eq!(state.instrument_target(), Some(tracks[2].id));
    assert_eq!(state.sixteen_levels_source_pitch(), None);
    assert!(state.choosing_sixteen_levels_source());
}

#[test]
fn range_edits_apply_to_the_next_note_immediately() {
    let track_id = TrackId::new();
    let mut state = PerformState::default();
    state.resolve_instrument_note(position(0), 64, track_id);
    state.toggle_sixteen_levels();
    state.set_sixteen_levels_minimum(-12);
    state.set_sixteen_levels_maximum(12);

    let pitch_low = state.resolve_instrument_note(position(0), 64, track_id);
    let pitch_high = state.resolve_instrument_note(position(15), 64, track_id);
    assert_eq!((pitch_low.pitch, pitch_high.pitch), (24, 48));

    state.select_sixteen_levels_parameter(SixteenLevelsParameter::Velocity);
    state.set_sixteen_levels_minimum(20);
    state.set_sixteen_levels_maximum(80);
    let velocity_low = state.resolve_instrument_note(position(0), 3, track_id);
    let velocity_high = state.resolve_instrument_note(position(15), 120, track_id);
    assert_eq!((velocity_low.velocity, velocity_high.velocity), (20, 80));
}

#[test]
fn range_edits_clamp_to_descriptor_bounds_and_never_cross() {
    let mut state = PerformState::default();
    state.set_sixteen_levels_minimum(500);
    assert_eq!(
        state.sixteen_levels_range(),
        SixteenLevelsRange {
            minimum: 15,
            maximum: 15
        }
    );
    state.set_sixteen_levels_maximum(-500);
    assert_eq!(state.sixteen_levels_range().maximum, 15);

    state.select_sixteen_levels_parameter(SixteenLevelsParameter::Velocity);
    state.set_sixteen_levels_minimum(-1);
    state.set_sixteen_levels_maximum(500);
    assert_eq!(
        state.sixteen_levels_range(),
        SixteenLevelsRange {
            minimum: 1,
            maximum: 127
        }
    );
}

#[test]
fn perform_messages_send_transformed_notes_through_the_existing_engine_path() {
    let mut track = ProjectTrack::new(TrackId::new(), "Drums".into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    track.has_instrument = true;
    let tracks = vec![track];
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(tracks[0].id),
    };
    let mut state = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let now = std::time::Instant::now();

    state.update(PerformMsg::ToggleFullLevel, &mut engine, ctx);
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: now,
        },
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "z".into(),
            occurred_at: now,
        },
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::ToggleSixteenLevels, &mut engine, ctx);
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Digit4,
            key_id: "4".into(),
            occurred_at: now,
        },
        &mut engine,
        ctx,
    );

    assert!(matches!(
        engine.0.as_slice(),
        [
            vibez_engine::commands::EngineCommand::ExternalNoteOn {
                pitch: 36,
                velocity: 127,
                ..
            },
            vibez_engine::commands::EngineCommand::ExternalNoteOff { pitch: 36, .. },
            vibez_engine::commands::EngineCommand::ExternalNoteOn {
                pitch: 51,
                velocity: 127,
                ..
            },
        ]
    ));
}
