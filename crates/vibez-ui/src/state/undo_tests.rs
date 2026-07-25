use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};

use crate::domains::arrangement::{ArrangementCtx, ArrangementMsg};
use crate::domains::perform::{PerformCtx, PerformMsg};
use crate::domains::test_support::RecordingEngine;

use super::{
    AppState, ArrangementTimeline, ProjectSnapshot, ProjectTrack, TrackTimelineContent, UiClip,
    UiEffect, UiNoteClip, UndoGestureId,
};

fn snapshot(state: &AppState) -> ProjectSnapshot {
    ProjectSnapshot {
        project_tracks: Arc::clone(&state.project_tracks),
        arrange_timeline: Arc::clone(&state.arrangement.timeline),
        sections: Arc::clone(&state.perform.sections),
        bpm: state.transport.bpm,
        project_swing: state.perform.project_swing(),
        loop_enabled: state.transport.loop_enabled,
        loop_start_beats: state.transport.loop_start_beats,
        loop_end_beats: state.transport.loop_end_beats,
    }
}

fn apply_snapshot(state: &mut AppState, snapshot: ProjectSnapshot) {
    state.project_tracks = snapshot.project_tracks;
    state.arrangement.timeline = snapshot.arrange_timeline;
    state.perform.sections = snapshot.sections;
    state.transport.bpm = snapshot.bpm;
    state.transport.bpm_text = format!("{:.0}", snapshot.bpm);
    state.perform.set_project_swing(snapshot.project_swing);
    state.transport.loop_enabled = snapshot.loop_enabled;
    state.transport.loop_start_beats = snapshot.loop_start_beats;
    state.transport.loop_end_beats = snapshot.loop_end_beats;
}

fn perform_edit(
    state: &mut AppState,
    engine: &mut RecordingEngine,
    message: PerformMsg,
) -> crate::domains::perform::PerformAction {
    state.project.history.push_edit(snapshot(state), None);
    state.perform.update(
        message,
        engine,
        PerformCtx {
            workspace_visible: true,
            project_tracks: &state.project_tracks.tracks,
            selected_project_track: state.arrangement.selected_track,
        },
    )
}

fn undo_once(state: &mut AppState) {
    let snapshot = state.project.history.pop_undo().unwrap();
    apply_snapshot(state, snapshot);
}

#[test]
fn one_eq_drag_is_one_undo_step() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    let mut track = ProjectTrack::new(track_id, "Audio".into(), 0);
    track.effects.push(UiEffect {
        id: effect_id,
        effect_type: EffectType::Eq,
        bypass: false,
        params: vec![0.0],
        descriptors: &[],
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
    });
    Arc::make_mut(&mut state.project_tracks).tracks.push(track);

    let gesture = UndoGestureId::new();
    for gain in [1.0, 2.0, 3.0] {
        state
            .project
            .history
            .push_edit(snapshot(&state), Some(gesture));
        Arc::make_mut(&mut state.project_tracks).tracks[0].effects[0].params[0] = gain;
    }

    assert_eq!(state.project.history.undo.len(), 1);
    let before_drag = state.project.history.pop_undo().unwrap();
    apply_snapshot(&mut state, before_drag);
    assert_eq!(state.project_tracks.tracks[0].effects[0].params[0], 0.0);
}

#[test]
fn separate_drags_are_separate_undo_steps() {
    let mut state = AppState::default();
    let first_drag = UndoGestureId::new();
    let second_drag = UndoGestureId::new();

    state
        .project
        .history
        .push_edit(snapshot(&state), Some(first_drag));
    state
        .project
        .history
        .push_edit(snapshot(&state), Some(first_drag));
    state
        .project
        .history
        .push_edit(snapshot(&state), Some(second_drag));
    state
        .project
        .history
        .push_edit(snapshot(&state), Some(second_drag));

    assert_eq!(state.project.history.undo.len(), 2);
}

#[test]
fn clip_resize_does_not_hide_the_preceding_automation_undo_step() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "MIDI".into(), 0));
    state.arrangement.timeline = Arc::new(ArrangementTimeline {
        by_track: [(
            track_id,
            TrackTimelineContent {
                note_clips: vec![UiNoteClip {
                    id: clip_id,
                    name: "Pattern".into(),
                    position_beats: 0.0,
                    duration_beats: 4.0,
                    notes: Vec::new(),
                    selected_notes: Default::default(),
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                    groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
                }],
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
    });

    state.project.history.push_edit(snapshot(&state), None);
    Arc::make_mut(&mut state.arrangement.timeline)
        .get_mut(track_id)
        .unwrap()
        .automation
        .push(AutomationLane::new(AutomationTarget::TrackGain));

    let resize = UndoGestureId::new();
    for duration in [5.0, 6.0, 8.0] {
        state
            .project
            .history
            .push_edit(snapshot(&state), Some(resize));
        Arc::make_mut(&mut state.arrangement.timeline)
            .get_mut(track_id)
            .unwrap()
            .note_clips[0]
            .duration_beats = duration;
    }

    let before_resize = state.project.history.pop_undo().unwrap();
    apply_snapshot(&mut state, before_resize);
    let content = state.arrangement.timeline.get(track_id).unwrap();
    assert_eq!(content.note_clips[0].duration_beats, 4.0);
    assert_eq!(content.automation.len(), 1);

    let before_automation = state.project.history.pop_undo().unwrap();
    apply_snapshot(&mut state, before_automation);
    assert!(state
        .arrangement
        .timeline
        .get(track_id)
        .unwrap()
        .automation
        .is_empty());
}

#[test]
fn section_crud_operations_are_individual_undo_steps() {
    let mut state = AppState::default();
    let mut engine = RecordingEngine::default();

    perform_edit(&mut state, &mut engine, PerformMsg::CreateSectionAt(0));
    let source_id = state.perform.selected_section.unwrap();
    state.perform.section_name_edit = "Breakdown".into();
    perform_edit(
        &mut state,
        &mut engine,
        PerformMsg::CommitSectionName(source_id),
    );
    state.perform.duplicate_source = Some(source_id);
    perform_edit(&mut state, &mut engine, PerformMsg::DuplicateSectionTo(4));
    let duplicate_id = state.perform.selected_section.unwrap();
    perform_edit(
        &mut state,
        &mut engine,
        PerformMsg::DeleteSection(duplicate_id),
    );

    assert_eq!(state.project.history.undo.len(), 4);
    assert_eq!(state.perform.sections.sections.len(), 1);

    undo_once(&mut state);
    assert_eq!(state.perform.sections.sections.len(), 2);
    assert_eq!(state.perform.sections.by_id(duplicate_id).unwrap().slot, 4);

    undo_once(&mut state);
    assert_eq!(state.perform.sections.sections.len(), 1);
    assert_eq!(
        state.perform.sections.by_id(source_id).unwrap().name,
        "Breakdown"
    );

    undo_once(&mut state);
    assert_eq!(
        state.perform.sections.by_id(source_id).unwrap().name,
        "Section 01"
    );

    undo_once(&mut state);
    assert!(state.perform.sections.sections.is_empty());
    assert!(engine.0.is_empty());
}

#[test]
fn project_and_track_swing_edits_restore_through_undo() {
    let mut state = AppState::default();
    let mut engine = RecordingEngine::default();
    let mut track = ProjectTrack::new(TrackId::new(), "Hats".into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    track.has_instrument = true;
    let track_id = track.id;
    Arc::make_mut(&mut state.project_tracks).tracks.push(track);
    state.arrangement.selected_track = Some(track_id);

    perform_edit(&mut state, &mut engine, PerformMsg::SetProjectSwing(0.63));
    let action = perform_edit(
        &mut state,
        &mut engine,
        PerformMsg::SetTrackSwingOffset {
            track_id,
            value: Some(0.08),
        },
    );
    let request = action.track_swing_request.expect("track Swing edit");
    Arc::make_mut(&mut state.project_tracks)
        .find_mut(request.track_id)
        .unwrap()
        .swing_offset = request.swing_offset;

    assert_eq!(state.project.history.undo.len(), 2);
    assert_eq!(state.perform.project_swing().get(), 0.63);
    assert_eq!(
        state.project_tracks.find(track_id).unwrap().swing_offset,
        Some(vibez_core::perform::SwingOffset::new(0.08))
    );

    undo_once(&mut state);
    assert_eq!(
        state.project_tracks.find(track_id).unwrap().swing_offset,
        None
    );
    assert_eq!(state.perform.project_swing().get(), 0.63);

    undo_once(&mut state);
    assert_eq!(
        state.perform.project_swing(),
        vibez_core::perform::SwingAmount::default()
    );
}

#[test]
fn project_track_deletion_across_timelines_is_one_undo_step() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Shared".into(), 0));
    Arc::make_mut(&mut state.arrangement.timeline)
        .ensure(track_id)
        .automation
        .push(AutomationLane::new(AutomationTarget::TrackGain));
    for slot in [0, 3] {
        let mut section = crate::domains::perform::Section::new(slot);
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .automation
            .push(AutomationLane::new(AutomationTarget::TrackPan));
        Arc::make_mut(&mut state.perform.sections).insert(section);
    }
    let mut engine = RecordingEngine::default();
    state.arrangement.update(
        Arc::make_mut(&mut state.project_tracks),
        ArrangementMsg::RequestRemoveTrack(track_id),
        &mut engine,
        ArrangementCtx::default(),
    );
    assert!(state
        .project
        .history
        .begin_transaction(snapshot(&state), state.project.dirty));
    state.project.history.push_edit(snapshot(&state), None);
    let action = state.arrangement.update(
        Arc::make_mut(&mut state.project_tracks),
        ArrangementMsg::ConfirmRemoveTrack(track_id),
        &mut engine,
        ArrangementCtx::default(),
    );
    Arc::make_mut(&mut state.perform.sections).remove_track(
        action
            .remove_track_from_sections
            .expect("confirmed deletion spans Sections"),
    );
    assert!(state.project.history.commit_transaction());

    assert_eq!(state.project.history.undo.len(), 1);
    assert!(state.project_tracks.find(track_id).is_none());
    assert!(state.arrangement.timeline.get(track_id).is_none());
    assert!(state
        .perform
        .sections
        .sections
        .iter()
        .all(|section| section.timeline.get(track_id).is_none()));

    let deleted = snapshot(&state);
    let before_deletion = state.project.history.pop_undo().unwrap();
    state.project.history.push_redo(deleted);
    apply_snapshot(&mut state, before_deletion);
    assert!(state.project_tracks.find(track_id).is_some());
    assert!(state.arrangement.timeline.get(track_id).is_some());
    assert!(state
        .perform
        .sections
        .sections
        .iter()
        .all(|section| section.timeline.get(track_id).is_some()));

    let restored = snapshot(&state);
    let after_deletion = state.project.history.pop_redo().unwrap();
    state.project.history.push_undo(restored);
    apply_snapshot(&mut state, after_deletion);
    assert!(state.project_tracks.find(track_id).is_none());
    assert!(state.arrangement.timeline.get(track_id).is_none());
    assert!(state
        .perform
        .sections
        .sections
        .iter()
        .all(|section| section.timeline.get(track_id).is_none()));
}

#[test]
fn transaction_snapshot_capture_shares_project_storage() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Shared".into(), 0));
    Arc::make_mut(&mut state.arrangement.timeline).ensure(track_id);
    Arc::make_mut(&mut state.perform.sections).insert(crate::domains::perform::Section::new(0));

    let captured = snapshot(&state);

    assert!(Arc::ptr_eq(&captured.project_tracks, &state.project_tracks));
    assert!(Arc::ptr_eq(
        &captured.arrange_timeline,
        &state.arrangement.timeline
    ));
    assert!(Arc::ptr_eq(&captured.sections, &state.perform.sections));
}

#[test]
fn abandoning_a_transaction_rolls_back_all_canonical_edits_without_history() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Shared".into(), 0));
    Arc::make_mut(&mut state.arrangement.timeline).ensure(track_id);
    let mut section = crate::domains::perform::Section::new(0);
    Arc::make_mut(&mut section.timeline).ensure(track_id);
    let section_id = section.id;
    Arc::make_mut(&mut state.perform.sections).insert(section);

    assert!(state
        .project
        .history
        .begin_transaction(snapshot(&state), state.project.dirty));
    state.project.history.push_edit(snapshot(&state), None);
    Arc::make_mut(&mut state.arrangement.timeline).remove(track_id);
    Arc::make_mut(&mut state.perform.sections)
        .by_id_mut(section_id)
        .unwrap()
        .timeline = Arc::new(ArrangementTimeline::default());

    state.project.dirty = true;
    let (before, dirty_before) = state
        .project
        .history
        .abandon_transaction()
        .expect("open transaction");
    apply_snapshot(&mut state, before);
    state.project.dirty = dirty_before;

    assert!(state.arrangement.timeline.get(track_id).is_some());
    assert!(state
        .perform
        .sections
        .by_id(section_id)
        .unwrap()
        .timeline
        .get(track_id)
        .is_some());
    assert!(state.project.history.undo.is_empty());
    assert!(state.project.history.redo.is_empty());
    assert!(!state.project.history.transaction_active());
    assert!(!state.project.dirty);
}

#[test]
fn undo_restores_canonical_state_without_restoring_selection() {
    let mut state = AppState::default();
    let first = TrackId::new();
    let second = TrackId::new();
    Arc::make_mut(&mut state.project_tracks).tracks.extend([
        ProjectTrack::new(first, "First".into(), 0),
        ProjectTrack::new(second, "Second".into(), 1),
    ]);
    state.arrangement.selected_track = Some(first);

    state.project.history.push_edit(snapshot(&state), None);
    Arc::make_mut(&mut state.project_tracks)
        .find_mut(first)
        .unwrap()
        .gain = 0.25;
    state.arrangement.selected_track = Some(second);

    undo_once(&mut state);

    assert_eq!(state.project_tracks.find(first).unwrap().gain, 1.0);
    assert_eq!(state.arrangement.selected_track, Some(second));
}

#[test]
fn empty_transaction_does_not_create_an_undo_step() {
    let mut state = AppState::default();
    assert!(state
        .project
        .history
        .begin_transaction(snapshot(&state), state.project.dirty));
    assert!(!state.project.history.commit_transaction());
    assert!(state.project.history.undo.is_empty());
}

#[test]
fn cut_and_each_paste_are_separate_undo_steps_while_clipboard_survives_undo() {
    let mut state = AppState::default();
    let track_id = TrackId::new();
    Arc::make_mut(&mut state.project_tracks)
        .tracks
        .push(ProjectTrack::new(track_id, "Audio".into(), 0));
    let source_id = ClipId::new();
    Arc::make_mut(&mut state.arrangement.timeline)
        .ensure(track_id)
        .clips
        .push(UiClip {
            id: source_id,
            name: "Source".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; 400]],
                sample_rate: 48_000,
            }),
            source: None,
            position: 200,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
            original_audio: None,
        });
    state.arrangement.selected_track = Some(track_id);
    state
        .arrangement
        .selected_clips
        .insert(crate::state::ArrangementSelection::AudioClip {
            track_id,
            clip_id: source_id,
        });
    let project_tracks = Arc::clone(&state.project_tracks);
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..ArrangementCtx::default()
    };
    let mut engine = RecordingEngine::default();

    let before_cut = snapshot(&state);
    let cut = state.arrangement.editor.update_clipboard(
        &project_tracks,
        ArrangementMsg::CutSelectedClips,
        &mut state.clip_clipboard,
        &mut engine,
        ctx,
    );
    assert!(cut.mark_dirty);
    state.project.history.push_edit(before_cut, None);
    assert!(state
        .arrangement
        .timeline
        .get(track_id)
        .unwrap()
        .clips
        .is_empty());

    let before_paste = snapshot(&state);
    let paste = state.arrangement.editor.update_clipboard(
        &project_tracks,
        ArrangementMsg::PasteClips,
        &mut state.clip_clipboard,
        &mut engine,
        ctx,
    );
    assert!(paste.mark_dirty);
    state.project.history.push_edit(before_paste, None);

    let before_second_paste = snapshot(&state);
    let second_paste = state.arrangement.editor.update_clipboard(
        &project_tracks,
        ArrangementMsg::PasteClips,
        &mut state.clip_clipboard,
        &mut engine,
        ctx,
    );
    assert!(second_paste.mark_dirty);
    state.project.history.push_edit(before_second_paste, None);
    assert_eq!(state.project.history.undo.len(), 3);
    assert_eq!(state.clip_clipboard.clips.len(), 1);

    undo_once(&mut state);
    assert_eq!(
        state
            .arrangement
            .timeline
            .get(track_id)
            .unwrap()
            .clips
            .len(),
        1
    );
    assert_eq!(state.clip_clipboard.clips.len(), 1);

    undo_once(&mut state);
    assert!(state
        .arrangement
        .timeline
        .get(track_id)
        .unwrap()
        .clips
        .is_empty());
    assert_eq!(state.clip_clipboard.clips.len(), 1);

    undo_once(&mut state);
    assert_eq!(
        state.arrangement.timeline.get(track_id).unwrap().clips[0].id,
        source_id
    );
    assert_eq!(state.clip_clipboard.clips.len(), 1);
}
