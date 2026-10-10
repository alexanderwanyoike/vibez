//! Bounded event delivery preserves state and recovers from UI backpressure.

use super::*;

#[test]
fn cancellation_delivers_applied_mute_and_stop_state_immediately() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    engine.present_event(
        EngineEvent::TrackMuteChanged {
            track_id,
            muted: true,
            effective_at_samples: 1000,
        },
        521,
    );
    engine.present_event(EngineEvent::PlaybackStopped, 521);
    engine.cancel_presentation();
    engine.flush_presentation();
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(event, EngineEvent::TrackMuteChanged { track_id: id, muted: true, .. } if *id == track_id)));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::PlaybackStopped)));
}

#[test]
fn full_presentation_queue_stops_and_can_restart_without_replacing_valid_plan() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine.transport.play();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event(EngineEvent::PlaybackStarted, 521);
    }
    engine.present_event(EngineEvent::PlaybackStarted, 521);
    assert!(engine.presentation_fault);
    assert!(!engine.transport.is_playing());
    assert!(engine.compensation_valid);
    let mut stopped = false;
    let mut failure = false;
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            stopped |= matches!(event, EngineEvent::PlaybackStopped);
            failure |= matches!(event, EngineEvent::CompensationInvalid { .. });
        }
        engine.flush_presentation();
    }
    assert!(stopped && failure);
    assert!(engine.scheduled_presentation.is_empty());
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    assert!(!engine.presentation_fault);
    assert!(engine.transport.is_playing());
    assert!(engine.compensation_valid);
}

#[test]
fn due_event_batch_is_bounded_by_free_ring_slots() {
    let (mut engine, _, mut events) = AudioEngine::new();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..64 {
        engine.present_event(EngineEvent::PlaybackStopped, 1);
    }
    engine.output_position = 1;
    for _ in 0..7 {
        events.pop().unwrap();
    }
    engine.flush_presentation();
    assert_eq!(engine.scheduled_presentation.len(), 57);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    assert!(engine.scheduled_presentation.is_empty());
}
