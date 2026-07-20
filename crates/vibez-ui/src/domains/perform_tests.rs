use super::super::test_support::RecordingEngine;
use super::*;

fn project_tracks(count: usize) -> Vec<ProjectTrack> {
    (0..count)
        .map(|index| ProjectTrack::new(TrackId::new(), format!("Track {}", index + 1), index as u8))
        .collect()
}

fn playable_midi_track(name: &str) -> ProjectTrack {
    let mut track = ProjectTrack::new(TrackId::new(), name.into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    track.has_instrument = true;
    track
}

#[test]
fn exposes_exactly_the_three_settled_v1_modes() {
    assert_eq!(
        PerformMode::ALL.map(PerformMode::label),
        ["Sections", "Track Mutes", "Instrument"]
    );
    assert_eq!(
        PerformMode::ALL.map(PerformMode::shortcut),
        ["F1", "F2", "F3"]
    );
}

#[test]
fn visible_mode_switches_are_ui_only() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();

    let action = state.update(
        PerformMsg::SelectMode(PerformMode::Instrument),
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            ..PerformCtx::default()
        },
    );

    assert_eq!(state.mode, PerformMode::Instrument);
    assert_eq!(action, PerformAction::default());
    assert!(engine.0.is_empty());
    assert!(!PerformMsg::SelectMode(PerformMode::Sections).marks_dirty());
}

#[test]
fn shortcuts_do_not_change_hidden_perform_state() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();

    state.update(
        PerformMsg::SelectMode(PerformMode::TrackMutes),
        &mut engine,
        PerformCtx {
            workspace_visible: false,
            ..PerformCtx::default()
        },
    );

    assert_eq!(state.mode, PerformMode::Sections);
    assert!(engine.0.is_empty());
}

#[test]
fn pad_positions_are_stable_with_mode_specific_order_origins() {
    let top_left = PadPosition::ALL[0];
    let bottom_left = PadPosition::ALL[12];

    assert_eq!(top_left.ordinal(PerformMode::Sections), 1);
    assert_eq!(bottom_left.ordinal(PerformMode::Sections), 13);
    assert_eq!(top_left.ordinal(PerformMode::Instrument), 13);
    assert_eq!(bottom_left.ordinal(PerformMode::Instrument), 1);

    let mut instrument_ordinals =
        PadPosition::ALL.map(|position| position.ordinal(PerformMode::Instrument));
    instrument_ordinals.sort_unstable();
    assert_eq!(
        instrument_ordinals,
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    );
}

#[test]
fn bank_selection_and_focus_default_to_ui_owned_shell_state() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    assert_eq!(state.banks, PerformBanks::default());
    assert_eq!(state.selected_pad, None);
    assert_eq!(state.editor_focus, PerformEditorFocus::PadSurface);

    state.update(
        PerformMsg::FocusEditor(PerformEditorFocus::SectionConstruction),
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            ..PerformCtx::default()
        },
    );
    assert_eq!(state.editor_focus, PerformEditorFocus::SectionConstruction);
    assert!(engine.0.is_empty());
}

#[test]
fn default_mapping_uses_the_settled_physical_layout() {
    let mapping = PerformInputMapping::default();
    assert_eq!(
        PadPosition::ALL.map(|position| mapping.key_for(position).label()),
        ["1", "2", "3", "4", "Q", "W", "E", "R", "A", "S", "D", "F", "Z", "X", "C", "V"]
    );
}

#[test]
fn one_hold_produces_exactly_one_press_and_release() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    let pressed_at = Instant::now();
    let released_at = pressed_at + std::time::Duration::from_millis(23);
    let ctx = PerformCtx {
        workspace_visible: true,
        ..PerformCtx::default()
    };

    let press = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Q,
            key_id: "q".into(),
            occurred_at: pressed_at,
        },
        &mut engine,
        ctx,
    );
    let repeat = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Q,
            key_id: "q".into(),
            occurred_at: pressed_at,
        },
        &mut engine,
        ctx,
    );
    let release = state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "q".into(),
            occurred_at: released_at,
        },
        &mut engine,
        ctx,
    );
    let extra_release = state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "q".into(),
            occurred_at: released_at,
        },
        &mut engine,
        ctx,
    );

    let position = PadPosition { row: 1, column: 0 };
    assert_eq!(
        press.gesture,
        Some(PadGesture {
            position,
            kind: PadGestureKind::Press,
            velocity: None,
            source: PadGestureSource::ComputerKeyboard {
                key: ComputerKey::Q
            },
            occurred_at: pressed_at,
        })
    );
    assert!(repeat.keyboard_consumed);
    assert!(repeat.gesture.is_none());
    assert_eq!(release.gesture.unwrap().kind, PadGestureKind::Release);
    assert_eq!(release.gesture.unwrap().occurred_at, released_at);
    assert!(extra_release.gesture.is_none());
    assert!(!state.is_pad_pressed(position));
    assert!(engine.0.is_empty());
}

#[test]
fn instrument_pad_hold_plays_and_releases_the_selected_target() {
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
                velocity: 100,
            },
            vibez_engine::commands::EngineCommand::ExternalNoteOff {
                track_id: released_track,
                pitch: 36,
            },
        ] if *track_id == tracks[0].id && *released_track == tracks[0].id
    ));
}

#[test]
fn held_note_release_stays_paired_when_the_target_and_mode_change() {
    let tracks = vec![playable_midi_track("Drums"), playable_midi_track("Surge")];
    let mut state = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let at = Instant::now();
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: Some(tracks[0].id),
        },
    );
    state.mode = PerformMode::Sections;
    state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "z".into(),
            occurred_at: at,
        },
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: Some(tracks[1].id),
        },
    );

    assert!(matches!(
        engine.0.as_slice(),
        [
            vibez_engine::commands::EngineCommand::ExternalNoteOn { track_id, .. },
            vibez_engine::commands::EngineCommand::ExternalNoteOff {
                track_id: released_track,
                ..
            }
        ] if *track_id == tracks[0].id && *released_track == tracks[0].id
    ));
}

#[test]
fn shift_overlay_selects_only_playable_midi_targets_in_bottom_left_order() {
    let mut tracks = project_tracks(1);
    let first = playable_midi_track("Drums");
    let first_id = first.id;
    tracks.push(first);
    let mut uninstrumented = playable_midi_track("Empty MIDI");
    uninstrumented.has_instrument = false;
    tracks.push(uninstrumented);
    let second = playable_midi_track("Dexed");
    let second_id = second.id;
    tracks.push(second);
    let mut state = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(first_id),
    };

    state.update(
        PerformMsg::SetInstrumentTargetOverlay(true),
        &mut engine,
        ctx,
    );
    let action = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::X,
            key_id: "x".into(),
            occurred_at: Instant::now(),
        },
        &mut engine,
        ctx,
    );

    assert_eq!(action.select_project_track, Some(second_id));
    assert!(engine.0.is_empty(), "target selection must not play a note");
}

#[test]
fn shift_release_clears_the_overlay_after_focus_leaves_perform() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::SetInstrumentTargetOverlay(true),
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            ..PerformCtx::default()
        },
    );

    state.update(
        PerformMsg::SetInstrumentTargetOverlay(false),
        &mut engine,
        PerformCtx {
            workspace_visible: false,
            ..PerformCtx::default()
        },
    );

    assert!(!state.instrument_target_overlay);
}

#[test]
fn on_screen_target_selection_retargets_the_project_track() {
    let tracks = vec![playable_midi_track("Surge")];
    let mut state = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();

    let action = state.update(
        PerformMsg::SelectInstrumentTarget(tracks[0].id),
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: None,
        },
    );

    assert_eq!(action.select_project_track, Some(tracks[0].id));
    assert!(engine.0.is_empty());
}

#[test]
fn changing_fixed_velocity_persists_and_affects_the_next_note_immediately() {
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

    let setting = state.update(PerformMsg::SetFixedComputerVelocity(73), &mut engine, ctx);
    state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Z,
            key_id: "z".into(),
            occurred_at: Instant::now(),
        },
        &mut engine,
        ctx,
    );

    assert!(setting.persist_settings);
    assert!(matches!(
        engine.0.as_slice(),
        [vibez_engine::commands::EngineCommand::ExternalNoteOn { velocity: 73, .. }]
    ));
}

#[test]
fn mapping_changes_do_not_change_gesture_structure_between_modes() {
    let at = Instant::now();
    let mut engine = RecordingEngine::default();
    let mut sections = PerformState::default();
    let mut instrument = PerformState {
        mode: PerformMode::Instrument,
        ..PerformState::default()
    };
    sections
        .input_mapping
        .rebind(PadPosition::ALL[0], ComputerKey::Y);
    instrument.input_mapping = sections.input_mapping.clone();

    let mut press = |state: &mut PerformState, key_id: &str| {
        state
            .update(
                PerformMsg::ComputerKeyPressed {
                    key: ComputerKey::Y,
                    key_id: key_id.into(),
                    occurred_at: at,
                },
                &mut engine,
                PerformCtx {
                    workspace_visible: true,
                    ..PerformCtx::default()
                },
            )
            .gesture
            .unwrap()
    };

    assert_eq!(
        press(&mut sections, "sections"),
        press(&mut instrument, "instrument")
    );
}

#[test]
fn release_keeps_the_original_pair_when_mapping_changes_mid_hold() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        ..PerformCtx::default()
    };
    let at = Instant::now();
    let press = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Q,
            key_id: "q".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );
    state
        .input_mapping
        .rebind(PadPosition::ALL[0], ComputerKey::Q);
    let release = state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "q".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );

    assert_eq!(press.gesture.unwrap().position, PadPosition::ALL[4]);
    assert_eq!(release.gesture.unwrap().position, PadPosition::ALL[4]);
}

#[test]
fn rebinding_swaps_an_existing_key_and_requests_global_persistence() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    state.update(
        PerformMsg::BeginKeyRebind(PadPosition::ALL[0]),
        &mut engine,
        PerformCtx::default(),
    );
    let action = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Q,
            key_id: "q".into(),
            occurred_at: Instant::now(),
        },
        &mut engine,
        PerformCtx::default(),
    );

    assert_eq!(
        state.input_mapping.key_for(PadPosition::ALL[0]),
        ComputerKey::Q
    );
    assert_eq!(
        state.input_mapping.key_for(PadPosition::ALL[4]),
        ComputerKey::Digit1
    );
    assert!(action.keyboard_consumed);
    assert!(action.persist_settings);
    assert!(action.gesture.is_none());
}

#[test]
fn section_crud_and_properties_update_the_ordered_store() {
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        ..PerformCtx::default()
    };

    state.update(PerformMsg::CreateSectionAt(5), &mut engine, ctx);
    let source_id = state.selected_section.expect("new section selected");
    state.update(
        PerformMsg::StartEditingSectionName(source_id),
        &mut engine,
        ctx,
    );
    assert_eq!(state.editing_section_name, Some(source_id));
    state.update(
        PerformMsg::SectionNameInput("Breakdown".into()),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::CommitSectionName(source_id), &mut engine, ctx);
    assert_eq!(state.editing_section_name, None);
    state.update(
        PerformMsg::SetSectionLengthBeats(source_id, 32.0),
        &mut engine,
        ctx,
    );
    state.update(
        PerformMsg::SetSectionLaunchQuantization(
            source_id,
            SectionLaunchQuantization::EndOfSection,
        ),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::ToggleSectionLoop(source_id), &mut engine, ctx);
    state.update(
        PerformMsg::BeginDuplicateSection(source_id),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::DuplicateSectionTo(2), &mut engine, ctx);

    let duplicate_id = state.selected_section.expect("duplicate selected");
    let duplicate = state.sections.by_id(duplicate_id).unwrap();
    assert_eq!(duplicate.slot, 2);
    assert_eq!(duplicate.name, "Breakdown Copy");
    assert_eq!(duplicate.length_beats, 32.0);
    assert_eq!(
        duplicate.launch_quantization,
        SectionLaunchQuantization::EndOfSection
    );
    assert!(!duplicate.looping);
    assert_eq!(state.sections.sections[1].slot, 5);

    state.update(PerformMsg::DeleteSection(duplicate_id), &mut engine, ctx);
    assert!(state.sections.by_id(duplicate_id).is_none());
    assert_eq!(state.selected_section, None);
    assert!(engine.0.is_empty());
}

#[test]
fn selecting_a_section_preserves_project_track_selection_and_resets_clip_focus() {
    let tracks = project_tracks(2);
    let selected_track = tracks[1].id;
    let mut state = PerformState::default();
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(selected_track),
    };

    state.update(PerformMsg::CreateSectionAt(0), &mut engine, ctx);
    state.section_editor.editor_mut().selected_note_clip =
        Some((selected_track, vibez_core::id::ClipId::new()));
    let second = Section::new(1);
    let second_id = second.id;
    Arc::make_mut(&mut state.sections).insert(second);

    state.update(PerformMsg::SelectSection(second_id), &mut engine, ctx);

    assert_eq!(state.selected_section, Some(second_id));
    assert_eq!(
        state.section_editor.editor().selected_track,
        Some(selected_track)
    );
    assert_eq!(state.section_editor.editor().selected_note_clip, None);
    assert!(engine.0.is_empty());
}

#[test]
fn keyboard_and_pointer_launch_select_the_target_then_allow_independent_selection() {
    let first = Section::new(0);
    let first_id = first.id;
    let second = Section::new(1);
    let second_id = second.id;
    let mut state = PerformState::default();
    Arc::make_mut(&mut state.sections).insert(first);
    Arc::make_mut(&mut state.sections).insert(second);
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        ..PerformCtx::default()
    };

    let pointer = state.update(PerformMsg::LaunchSection(first_id), &mut engine, ctx);
    assert_eq!(state.selected_section, Some(first_id));
    let keyboard = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Digit1,
            key_id: "1".into(),
            occurred_at: Instant::now(),
        },
        &mut engine,
        ctx,
    );
    assert_eq!(state.selected_section, Some(first_id));
    let selection = state.update(PerformMsg::SelectSection(second_id), &mut engine, ctx);

    assert_eq!(pointer.section_launch, Some(first_id));
    assert_eq!(keyboard.section_launch, pointer.section_launch);
    assert_eq!(selection.section_launch, None);
    assert_eq!(state.selected_section, Some(second_id));
    assert_eq!(
        state.playing_section, None,
        "only engine events set playback truth"
    );
    assert_eq!(state.queued_section, None);
    assert_eq!(state.pending_section_boundary_samples, None);
}

#[test]
fn playback_affecting_section_edits_request_a_source_refresh() {
    let section = Section::new(0);
    let section_id = section.id;
    let mut state = PerformState::default();
    Arc::make_mut(&mut state.sections).insert(section);
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        ..PerformCtx::default()
    };

    let action = state.update(
        PerformMsg::SetSectionLengthBeats(section_id, 8.0),
        &mut engine,
        ctx,
    );

    assert_eq!(action.section_content_changed, Some(section_id));
}

#[test]
fn removing_track_content_only_changes_the_selected_section() {
    let tracks = project_tracks(1);
    let track_id = tracks[0].id;
    let mut first = Section::new(0);
    let mut second = Section::new(1);
    for section in [&mut first, &mut second] {
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .automation
            .push(vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackGain,
            ));
    }
    let first_id = first.id;
    let second_id = second.id;
    let mut state = PerformState::default();
    Arc::make_mut(&mut state.sections).insert(first);
    Arc::make_mut(&mut state.sections).insert(second);
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: Some(track_id),
    };
    state.update(PerformMsg::SelectSection(first_id), &mut engine, ctx);
    state.update(
        PerformMsg::RemoveTrackContent {
            section_id: first_id,
            track_id,
        },
        &mut engine,
        ctx,
    );

    assert_eq!(tracks.len(), 1);
    assert!(state
        .sections
        .by_id(first_id)
        .unwrap()
        .timeline
        .get(track_id)
        .is_none());
    assert!(state
        .sections
        .by_id(second_id)
        .unwrap()
        .timeline
        .get(track_id)
        .is_some());
    assert!(engine.0.is_empty());
}

#[test]
fn track_mute_mode_resolves_keyboard_press_to_shared_track_request_once() {
    let tracks = project_tracks(2);
    let mut state = PerformState {
        mode: PerformMode::TrackMutes,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let at = Instant::now();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: None,
    };

    let press = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Digit1,
            key_id: "1".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );
    let repeat = state.update(
        PerformMsg::ComputerKeyPressed {
            key: ComputerKey::Digit1,
            key_id: "1".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );
    let release = state.update(
        PerformMsg::ComputerKeyReleased {
            key_id: "1".into(),
            occurred_at: at,
        },
        &mut engine,
        ctx,
    );

    assert_eq!(
        press.track_mute_request,
        Some(TrackMuteRequest {
            track_id: tracks[0].id,
            muted: true,
        })
    );
    assert!(repeat.track_mute_request.is_none());
    assert!(release.track_mute_request.is_none());
    assert!(engine.0.is_empty());
}

#[test]
fn only_section_document_edits_are_dirty() {
    let id = SectionId::new();
    assert!(PerformMsg::CreateSectionAt(0).marks_dirty());
    assert!(PerformMsg::DuplicateSectionTo(1).marks_dirty());
    assert!(PerformMsg::DeleteSection(id).marks_dirty());
    assert!(PerformMsg::CommitSectionName(id).marks_dirty());
    assert!(PerformMsg::SetSectionLengthBeats(id, 8.0).marks_dirty());
    assert!(
        PerformMsg::SetSectionLaunchQuantization(id, SectionLaunchQuantization::OneBeat)
            .marks_dirty()
    );
    assert!(PerformMsg::ToggleSectionLoop(id).marks_dirty());
    assert!(PerformMsg::RemoveTrackContent {
        section_id: id,
        track_id: TrackId::new(),
    }
    .marks_dirty());
    assert!(!PerformMsg::SelectSection(id).marks_dirty());
    assert!(!PerformMsg::SectionNameInput("Draft".into()).marks_dirty());
    assert!(!PerformMsg::StartEditingSectionName(id).marks_dirty());
    assert!(!PerformMsg::CancelSectionNameEdit.marks_dirty());
    assert!(!PerformMsg::BeginDuplicateSection(id).marks_dirty());
}

#[test]
fn pointer_pad_uses_the_same_track_mute_resolution() {
    let mut tracks = project_tracks(1);
    tracks[0].mute = true;
    let mut state = PerformState {
        mode: PerformMode::TrackMutes,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();

    let action = state.update(
        PerformMsg::ToggleTrackMuteFromPad(PadPosition::ALL[0]),
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: None,
        },
    );

    assert_eq!(
        action.track_mute_request,
        Some(TrackMuteRequest {
            track_id: tracks[0].id,
            muted: false,
        })
    );
}

#[test]
fn track_slots_survive_deletion_and_fill_without_scrambling_other_pads() {
    let mut tracks = project_tracks(18);
    let first_id = tracks[0].id;
    let second_id = tracks[1].id;
    let seventeenth_id = tracks[16].id;
    let mut state = PerformState {
        mode: PerformMode::TrackMutes,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();

    state.update(
        PerformMsg::NextBank,
        &mut engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &tracks,
            selected_project_track: None,
        },
    );
    assert_eq!(state.banks.track_mutes, 1);
    assert_eq!(
        state
            .track_for_mute_pad(PadPosition::ALL[0], &tracks)
            .map(|track| track.id),
        Some(seventeenth_id)
    );

    state.banks.track_mutes = 0;
    tracks.retain(|track| track.id != first_id);
    state.sync_project_tracks(&tracks);
    assert_eq!(
        state
            .track_for_mute_pad(PadPosition::ALL[1], &tracks)
            .map(|track| track.id),
        Some(second_id)
    );

    let replacement = ProjectTrack::new(TrackId::new(), "Replacement".into(), 3);
    let replacement_id = replacement.id;
    tracks.push(replacement);
    state.sync_project_tracks(&tracks);
    assert_eq!(
        state
            .track_for_mute_pad(PadPosition::ALL[0], &tracks)
            .map(|track| track.id),
        Some(replacement_id)
    );
    assert_eq!(
        state
            .track_for_mute_pad(PadPosition::ALL[1], &tracks)
            .map(|track| track.id),
        Some(second_id)
    );
}

#[test]
fn each_perform_mode_remembers_its_own_bank() {
    let mut tracks = project_tracks(18);
    for track in &mut tracks {
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
    }
    let mut state = PerformState::default();
    Arc::make_mut(&mut state.sections).insert(Section::new(0));
    Arc::make_mut(&mut state.sections).insert(Section::new(16));
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: None,
    };

    state.update(PerformMsg::NextBank, &mut engine, ctx);
    assert_eq!(state.banks.sections, 1);

    state.update(
        PerformMsg::SelectMode(PerformMode::TrackMutes),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::NextBank, &mut engine, ctx);
    assert_eq!(state.banks.track_mutes, 1);

    state.update(
        PerformMsg::SelectMode(PerformMode::Instrument),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::NextBank, &mut engine, ctx);
    assert_eq!(state.banks.instrument, 1);

    state.update(
        PerformMsg::SelectMode(PerformMode::Sections),
        &mut engine,
        ctx,
    );
    state.update(PerformMsg::PreviousBank, &mut engine, ctx);
    assert_eq!(state.banks.sections, 0);
    assert_eq!(state.banks.track_mutes, 1);
    assert_eq!(state.banks.instrument, 1);
    assert!(engine.0.is_empty(), "bank changes never touch playback");
}

#[test]
fn full_first_section_bank_can_open_an_empty_bank_and_create_section_seventeen() {
    let mut state = PerformState::default();
    for slot in 0..16 {
        Arc::make_mut(&mut state.sections).insert(Section::new(slot));
    }
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &[],
        selected_project_track: None,
    };

    state.update(PerformMsg::NextBank, &mut engine, ctx);
    assert_eq!(state.banks.sections, 1, "] should expose empty bank 2");

    state.update(PerformMsg::CreateSectionAt(16), &mut engine, ctx);
    assert!(state.sections.at_slot(16).is_some());

    state.update(PerformMsg::PreviousBank, &mut engine, ctx);
    assert_eq!(state.banks.sections, 0);
    assert!(engine.0.is_empty(), "bank changes never touch playback");
}
