use super::*;
use vibez_core::id::TrackId;
use vibez_core::perform::TrackMuteQuantization;

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

#[test]
fn one_beat_mute_becomes_effective_at_the_exact_engine_boundary() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();

    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBeat,
        })
        .unwrap();
    engine.process(&mut [0.0; 7], 1);

    let effective = std::iter::from_fn(|| event_rx.pop().ok()).find_map(|event| match event {
        EngineEvent::TrackMuteChanged {
            track_id: event_track,
            muted,
            effective_at_samples,
        } if event_track == track_id => Some((muted, effective_at_samples)),
        _ => None,
    });
    assert_eq!(effective, Some((true, 4)));
    assert!(engine.tracks()[0].mute);
}

#[test]
fn second_quantized_gesture_cancels_without_changing_effective_mute() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    for _ in 0..2 {
        cmd_tx
            .push(EngineCommand::QueueTrackMute {
                track_id,
                muted: true,
                quantization: TrackMuteQuantization::OneBeat,
            })
            .unwrap();
    }
    engine.process(&mut [0.0; 7], 1);

    let events: Vec<_> = std::iter::from_fn(|| event_rx.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::TrackMuteQueueCancelled {
            track_id: event_track
        } if *event_track == track_id
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        EngineEvent::TrackMuteChanged {
            track_id: event_track,
            ..
        } if *event_track == track_id
    )));
    assert!(!engine.tracks()[0].mute);
}

#[test]
fn quantized_choice_applies_immediately_while_stopped() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
        .unwrap();

    engine.process(&mut [0.0], 1);

    let events: Vec<_> = std::iter::from_fn(|| event_rx.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::TrackMuteChanged {
            track_id: event_track,
            muted: true,
            effective_at_samples: 0,
        } if *event_track == track_id
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::TrackMuteQueued { .. })));
    assert!(engine.tracks()[0].mute);
}

#[test]
fn one_bar_mute_uses_the_next_bar_boundary() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
        .unwrap();
    engine.process(&mut [0.0; 20], 1);

    assert!(
        std::iter::from_fn(|| event_rx.pop().ok()).any(|event| matches!(
            event,
            EngineEvent::TrackMuteChanged {
                track_id: event_track,
                muted: true,
                effective_at_samples: 16,
            } if event_track == track_id
        ))
    );
}

#[test]
fn one_bar_mute_at_an_arrangement_loop_end_applies_on_the_wrap() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetArrangementLoopRegion { start: 0, end: 16 })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetArrangementLoop(true))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
        .unwrap();
    for _ in 0..40 {
        engine.process(&mut [0.0], 1);
    }

    assert!(
        std::iter::from_fn(|| event_rx.pop().ok()).any(|event| matches!(
            event,
            EngineEvent::TrackMuteChanged {
                track_id: event_track,
                muted: true,
                effective_at_samples: 16,
            } if event_track == track_id
        )),
        "the loop wrap is the next traversable bar boundary"
    );
    assert!(engine.tracks()[0].mute);
}

#[test]
fn stopping_cancels_a_pending_quantized_mute() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Stop).unwrap();
    engine.process(&mut [0.0], 1);

    let events: Vec<_> = std::iter::from_fn(|| event_rx.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::TrackMuteQueueCancelled {
            track_id: event_track
        } if *event_track == track_id
    )));
    assert!(!engine.tracks()[0].mute);
}

#[test]
fn quantized_mute_on_a_boundary_still_reports_deferred_ownership_before_effective_state() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 4], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBeat,
        })
        .unwrap();
    engine.process(&mut [0.0], 1);

    let mute_events: Vec<_> = std::iter::from_fn(|| event_rx.pop().ok())
        .filter(|event| {
            matches!(
                event,
                EngineEvent::TrackMuteQueued { .. } | EngineEvent::TrackMuteChanged { .. }
            )
        })
        .collect();
    assert!(matches!(
        mute_events.as_slice(),
        [
            EngineEvent::TrackMuteQueued {
                track_id: queued_track,
                muted: true,
                effective_at_samples: 4,
            },
            EngineEvent::TrackMuteChanged {
                track_id: changed_track,
                muted: true,
                effective_at_samples: 4,
            }
        ] if *queued_track == track_id && *changed_track == track_id
    ));
}

#[test]
fn immediate_mixer_mute_cancels_a_pending_pad_change_before_applying() {
    let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
    let track_id = TrackId::new();
    cmd_tx.push(EngineCommand::SetSampleRate(8)).unwrap();
    cmd_tx.push(EngineCommand::SetBpm(120.0)).unwrap();
    cmd_tx
        .push(EngineCommand::AddTrack(track_id, "Track 1".into()))
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::QueueTrackMute {
            track_id,
            muted: true,
            quantization: TrackMuteQuantization::OneBar,
        })
        .unwrap();
    engine.process(&mut [0.0], 1);
    while event_rx.pop().is_ok() {}

    cmd_tx
        .push(EngineCommand::SetTrackMute(track_id, true))
        .unwrap();
    engine.process(&mut [0.0], 1);

    let mute_events: Vec<_> = std::iter::from_fn(|| event_rx.pop().ok())
        .filter(|event| {
            matches!(
                event,
                EngineEvent::TrackMuteQueueCancelled { .. } | EngineEvent::TrackMuteChanged { .. }
            )
        })
        .collect();
    assert!(matches!(
        mute_events.as_slice(),
        [
            EngineEvent::TrackMuteQueueCancelled {
                track_id: cancelled_track,
            },
            EngineEvent::TrackMuteChanged {
                track_id: changed_track,
                muted: true,
                ..
            }
        ] if *cancelled_track == track_id && *changed_track == track_id
    ));
}
