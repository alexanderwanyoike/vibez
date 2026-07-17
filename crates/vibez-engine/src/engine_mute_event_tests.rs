use super::*;
use vibez_core::id::TrackId;

#[test]
fn mute_change_reports_engine_effective_state_and_sample_time() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    let mut first_block = [0.0; 8];
    engine.process(&mut first_block, 2);
    while event_rx.pop().is_ok() {}
    assert_eq!(engine.transport().position(), 4);

    cmd_tx
        .push(EngineCommand::SetTrackMute(track_id, true))
        .unwrap();
    let mut second_block = [0.0; 8];
    engine.process(&mut second_block, 2);

    assert!(matches!(
        event_rx.pop(),
        Ok(EngineEvent::TrackMuteChanged {
            track_id: event_track,
            muted: true,
            effective_at_samples: 4,
        }) if event_track == track_id
    ));
}
