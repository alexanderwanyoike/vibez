use crate::domains::project::{ProjectCtx, ProjectMsg};
use crate::state::{AppState, ProjectSnapshot, ProjectTrack, UiEffect};
use std::sync::Arc;
use vibez_core::id::TrackId;
use vibez_core::{effect::EffectType, id::EffectId};

fn effect() -> UiEffect {
    UiEffect {
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![1.0],
        descriptors: &[],
        plugin_name: None,
        has_plugin_gui: false,
        plugin_ref: None,
        latency_samples: Some(480),
        sidechains: vec![],
        inactive_sidechains: vec![],
        external_inputs: vec![],
    }
}
fn restore(state: &mut AppState, snapshot: ProjectSnapshot) {
    state.project_tracks = snapshot.project_tracks;
    state.arrangement.timeline = snapshot.arrange_timeline;
    state.perform.sections = snapshot.sections;
    state.perform.clips = snapshot.launcher_clips;
}
fn history(state: &mut AppState, message: ProjectMsg) {
    let snapshot_now = state.project_snapshot();
    let action = state.project.update(message, ProjectCtx { snapshot_now });
    restore(state, action.apply_snapshot.unwrap());
}

#[test]
fn monitoring_mode_and_bypass_are_project_snapshot_edits_with_undo_and_redo() {
    let mut state = AppState::default();
    let mut track = ProjectTrack::new(TrackId::new(), "Track".into(), 0);
    track.effects.push(effect());
    Arc::make_mut(&mut state.project_tracks).tracks.push(track);
    let initial = state.project_snapshot();
    state.project.history.push_edit(initial.clone(), None);
    Arc::make_mut(&mut state.project_tracks).reduced_latency_monitoring = true;
    assert!(!initial.project_tracks.reduced_latency_monitoring);
    assert!(!Arc::ptr_eq(&initial.project_tracks, &state.project_tracks));
    let before_bypass = state.project_snapshot();
    state.project.history.push_edit(before_bypass, None);
    Arc::make_mut(&mut state.project_tracks).tracks[0].effects[0].bypass = true;
    history(&mut state, ProjectMsg::Undo);
    assert!(state.project_tracks.reduced_latency_monitoring);
    assert!(!state.project_tracks.tracks[0].effects[0].bypass);
    history(&mut state, ProjectMsg::Undo);
    assert!(!state.project_tracks.reduced_latency_monitoring);
    history(&mut state, ProjectMsg::Redo);
    assert!(state.project_tracks.reduced_latency_monitoring);
    assert!(!state.project_tracks.tracks[0].effects[0].bypass);
    history(&mut state, ProjectMsg::Redo);
    assert!(state.project_tracks.reduced_latency_monitoring);
    assert!(state.project_tracks.tracks[0].effects[0].bypass);
    assert_eq!(
        state.project_tracks.tracks[0].effects[0].latency_samples,
        Some(480)
    );
}

#[test]
fn snapshots_do_not_turn_monitoring_preference_into_arrange_or_perform_content() {
    let mut state = AppState::default();
    let before = state.project_snapshot();
    Arc::make_mut(&mut state.project_tracks).reduced_latency_monitoring = true;
    let after = state.project_snapshot();
    assert!(Arc::ptr_eq(
        &before.arrange_timeline,
        &after.arrange_timeline
    ));
    assert!(Arc::ptr_eq(&before.sections, &after.sections));
    assert!(Arc::ptr_eq(&before.launcher_clips, &after.launcher_clips));
    assert!(after.project_tracks.reduced_latency_monitoring);
}
