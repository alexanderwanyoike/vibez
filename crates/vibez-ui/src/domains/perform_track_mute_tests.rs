use super::super::test_support::RecordingEngine;
use super::*;

#[test]
fn pad_request_uses_the_persisted_quantization_choice() {
    let tracks = vec![ProjectTrack::new(TrackId::new(), "Bass".into(), 0)];
    let mut state = PerformState {
        mode: PerformMode::TrackMutes,
        ..PerformState::default()
    };
    let mut engine = RecordingEngine::default();
    let ctx = PerformCtx {
        workspace_visible: true,
        project_tracks: &tracks,
        selected_project_track: None,
    };

    let preference = state.update(
        PerformMsg::SetTrackMuteQuantization(TrackMuteQuantization::OneBar),
        &mut engine,
        ctx,
    );
    let gesture = state.update(
        PerformMsg::ToggleTrackMuteFromPad(PadPosition::ALL[0]),
        &mut engine,
        ctx,
    );

    assert!(preference.persist_settings);
    assert_eq!(
        gesture.track_mute_request,
        Some(TrackMuteRequest {
            track_id: tracks[0].id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
    );
}
