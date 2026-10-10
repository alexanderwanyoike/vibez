//! Clock namespace changes and cancellation preserve applied audible state.

use super::*;
use vibez_core::perform::NoteRepeatRate;
use vibez_core::{
    id::EffectId,
    routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode},
};

#[test]
fn entering_perform_discards_arrange_history_before_launch_or_capture() {
    let (mut engine, _, _) = AudioEngine::new();
    engine.sample_rate = 48000;
    engine.transport.set_bpm(120.0);
    let effect = EffectId::new();
    let model = [RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        sends: vec![],
        effects: vec![RoutingEffect {
            id: effect,
            inputs: vec![],
            assignments: vec![],
            inactive_inputs: vec![],
        }],
    }];
    let mut routing = crate::routing::PreparedRouting::prepare_compensated(
        &model,
        512,
        &[(
            RoutingNode {
                channel: TrackId::MASTER,
                stage: NodeStage::Effect(effect),
            },
            4096,
        )],
        &[],
        1,
    )
    .unwrap();
    let position = crate::compensation_clock::PresentationPosition {
        arrange: 1_000_000,
        perform: 1_000_000,
        section: None,
        section_id: None,
        section_length: 0,
        generation: 1,
    };
    routing.presentation.record(position, 4096);
    routing.presentation.record(
        crate::compensation_clock::PresentationPosition {
            arrange: 1_004_096,
            perform: 1_004_096,
            ..position
        },
        512,
    );
    engine.routing = Some(routing);
    engine.begin_performance_clock();
    assert_eq!(engine.heard_capture_position(), 0);
    assert_eq!(engine.next_achievable_boundary(64, 4.0), 96000);
}

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
fn local_device_failure_closes_capture_without_canceling_public_state() {
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.compensation_callback_heard_start = 500;
    let track_id = TrackId::new();
    engine.present_event(
        EngineEvent::TrackMuteChanged {
            track_id,
            muted: true,
            effective_at_samples: 1000,
        },
        521,
    );
    engine.close_capture_for_device_failure();
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: 500
        })
    ));
    engine.output_position = 521;
    engine.flush_presentation();
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(event, EngineEvent::TrackMuteChanged { track_id: id, muted: true, .. } if id == track_id)));
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

#[test]
fn overflow_retains_capture_owner_without_callback_allocation_or_destruction() {
    let (mut engine, _, mut events) = AudioEngine::new();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event(EngineEvent::PlaybackStarted, 521);
    }
    let offsets: Arc<[(TrackId, u32)]> = vec![(TrackId::new(), 521)].into();
    let event = EngineEvent::CaptureTimingRetired(Arc::clone(&offsets));
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.present_event(event, 0)),
        (0, 0)
    );
    assert_eq!(Arc::strong_count(&offsets), 2);
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.flush_presentation()),
        (0, 0)
    );
    assert!(std::iter::from_fn(|| events.pop().ok())
        .any(|event| matches!(event, EngineEvent::CaptureTimingRetired(_))));
    assert_eq!(Arc::strong_count(&offsets), 1);
}

#[test]
fn repeated_source_and_heard_events_share_zero_delay_backpressure() {
    let (mut engine, _, mut events) = AudioEngine::new();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    let track_id = TrackId::new();
    let raw = crate::events::SourceRecordingPosition {
        effective_at_samples: 521,
        ..Default::default()
    };
    let source = EngineEvent::SourceNoteRepeated {
        track_id,
        pitch: 60,
        velocity: 100,
        rate: NoteRepeatRate::Sixteenth,
        position: raw,
    };
    let heard = EngineEvent::NoteRepeated {
        track_id,
        pitch: 60,
        velocity: 100,
        rate: NoteRepeatRate::Sixteenth,
        effective_at_samples: 0,
        canonical_at_samples: 0,
        section_id: None,
        section_position_samples: None,
        canonical_section_position_samples: None,
    };
    assert!(presentation_queue::emit_repeated(
        source,
        heard,
        &mut engine.event_tx,
        &mut engine.scheduled_presentation,
        0,
        0
    ));
    assert_eq!(engine.scheduled_presentation.len(), 2);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    assert!(
        matches!(events.pop(),Ok(EngineEvent::SourceNoteRepeated { position,.. }) if position==raw)
    );
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::NoteRepeated {
            effective_at_samples: 0,
            ..
        })
    ));
}
