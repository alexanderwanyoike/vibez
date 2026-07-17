use std::sync::Arc;

use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};

use super::{
    AppState, ArrangementTimeline, ProjectSnapshot, ProjectTrack, TrackTimelineContent, UiEffect,
    UiNoteClip, UndoGestureId,
};

fn snapshot(state: &AppState) -> ProjectSnapshot {
    ProjectSnapshot {
        project_tracks: Arc::clone(&state.project_tracks),
        arrange_timeline: Arc::clone(&state.arrangement.timeline),
        bpm: state.transport.bpm,
        bpm_text: state.transport.bpm_text.clone(),
        loop_enabled: state.transport.loop_enabled,
        loop_start_beats: state.transport.loop_start_beats,
        loop_end_beats: state.transport.loop_end_beats,
        selected_track: state.arrangement.selected_track,
        selected_clips: state.arrangement.selected_clips.clone(),
        selected_note_clip: state.arrangement.selected_note_clip,
    }
}

fn apply_snapshot(state: &mut AppState, snapshot: ProjectSnapshot) {
    state.project_tracks = snapshot.project_tracks;
    state.arrangement.timeline = snapshot.arrange_timeline;
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
