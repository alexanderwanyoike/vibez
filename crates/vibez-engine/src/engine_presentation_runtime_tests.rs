//! Clock namespace changes and cancellation preserve applied audible state.

use super::*;
use crate::events::{NoteRepeatedEvent, SourceNoteRepeatedEvent, TrackMuteChangedEvent};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::perform::NoteRepeatRate;
use vibez_core::{
    id::EffectId,
    routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode},
};
use vibez_instruments::create_instrument;

#[test]
fn entering_perform_rebases_the_heard_clock_before_launch_or_capture() {
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
    engine.routing = routing;
    engine.project_routing_published = true;
    {
            let mut retirement_allowance = engine.pending_retirements.reserve(2).expect("fixture admits its original owner-return budget");
            engine.begin_performance_clock( &mut retirement_allowance)
        };
    assert_eq!(engine.heard_capture_position(), 0);
    assert_eq!(engine.next_achievable_boundary(64, 4.0), 96000);
}

#[test]
fn cancellation_delivers_applied_mute_and_stop_state_immediately() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
        EngineEvent::TrackMuteChanged(TrackMuteChangedEvent {
            track_id,
            muted: true,
            effective_at_samples: 1000,
        }),
        engine.output_position.saturating_add(521_u64),
     &mut retirement_allowance)
        };
    {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
        EngineEvent::PlaybackStopped,
        engine.output_position.saturating_add(521_u64),
     &mut retirement_allowance)
        };
    engine.cancel_presentation();
    engine.flush_presentation();
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(event, EngineEvent::TrackMuteChanged(TrackMuteChangedEvent { track_id: id, muted: true, .. }) if *id == track_id)));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::PlaybackStopped)));
}

#[test]
fn local_device_failure_closes_capture_without_canceling_public_state() {
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.compensation_callback_heard_start = 500;
    let track_id = TrackId::new();
    {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_after(
        EngineEvent::TrackMuteChanged(TrackMuteChangedEvent {
            track_id,
            muted: true,
            effective_at_samples: 1000,
        }),
        521,
     &mut retirement_allowance)
        };
    engine.close_capture_for_device_failure();
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: 500
        })
    ));
    engine.output_position = 521;
    engine.flush_presentation();
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(event, EngineEvent::TrackMuteChanged(TrackMuteChangedEvent { track_id: id, muted: true, .. }) if id == track_id)));
}

#[test]
fn full_presentation_queue_stops_and_can_restart_without_replacing_valid_plan() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine.transport.play();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(521_u64),
         &mut retirement_allowance)
        };
    }
    {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
        EngineEvent::PlaybackStarted,
        engine.output_position.saturating_add(521_u64),
     &mut retirement_allowance)
        };
    assert!(engine.presentation_state.is_faulted());
    assert!(!engine.transport.is_playing());
    assert!(engine.compensation_plan.applied_plan_is_valid());
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
    assert!(!engine.presentation_state.is_faulted());
    assert!(engine.transport.is_playing());
    assert!(engine.compensation_plan.applied_plan_is_valid());
}

#[test]
fn due_event_batch_is_bounded_by_free_ring_slots() {
    let (mut engine, _, mut events) = AudioEngine::new();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..64 {
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
            EngineEvent::PlaybackStopped,
            engine.output_position.saturating_add(1_u64),
         &mut retirement_allowance)
        };
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
        {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_after(EngineEvent::PlaybackStarted, 521, &mut retirement_allowance)
        };
    }
    let offsets: Arc<[(TrackId, u32)]> = vec![(TrackId::new(), 521)].into();
    let event = EngineEvent::CaptureTimingRetired(Arc::clone(&offsets));
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event(event, &mut retirement_allowance)
        }),
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
    let source = EngineEvent::SourceNoteRepeated(SourceNoteRepeatedEvent {
        track_id,
        pitch: 60,
        velocity: 100,
        rate: NoteRepeatRate::Sixteenth,
        position: raw,
    });
    let heard = EngineEvent::NoteRepeated(NoteRepeatedEvent {
        track_id,
        pitch: 60,
        velocity: 100,
        rate: NoteRepeatRate::Sixteenth,
        effective_at_samples: 0,
        canonical_at_samples: 0,
        section_id: None,
        section_position_samples: None,
        canonical_section_position_samples: None,
    });
    assert!(presentation_queue::emit_repeated(
        source,
        heard,
        &mut engine.event_tx,
        &mut engine.scheduled_presentation,
        0,
        0,
        0
    ));
    assert_eq!(engine.scheduled_presentation.len(), 2);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    assert!(
        matches!(events.pop(),Ok(EngineEvent::SourceNoteRepeated(SourceNoteRepeatedEvent { position,.. })) if position==raw)
    );
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::NoteRepeated(NoteRepeatedEvent {
            effective_at_samples: 0,
            ..
        }))
    ));
}

#[test]
fn graph_repeat_overflow_closes_capture_during_playback_and_idle() {
    use crate::note_repeat::NoteRepeatClock;
    use vibez_core::midi::InstrumentKind;
    use vibez_core::perform::NoteRepeatRate;
    for idle in [false, true] {
        let track_id = TrackId::new();
        let (mut engine, _, _) =
            graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
        engine.sample_rate = 100;
        engine.transport.set_bpm(60.0);
        let mut track = EngineTrack::new(track_id);
        track.instrument = Some(create_instrument(InstrumentKind::SubtractiveSynth, 100.0));
        engine.tracks.push(Box::new(track));
        engine.render_idle_instruments(&mut [0.0; 16], 8, 2, 0, None, None);
        engine.tracks[0].start_note_repeat(
            NoteRepeatStart {
                id: 0,
                pitch: 42,
                velocity: 100,
                rate: NoteRepeatRate::Sixteenth,
            },
            NoteRepeatClock {
                after_sample: 0,
                anchor_sample: 0,
                include_after_sample: true,
                bpm: 60.0,
                sample_rate: 100,
                swing: SwingAmount::default(),
            },
        );
        while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
            {
            let mut retirement_allowance = engine.pending_retirements.reserve(1).expect("fixture admits its original owner-return budget");
            engine.present_event_at(
                EngineEvent::PlaybackStarted,
                engine.output_position.saturating_add(521_u64),
             &mut retirement_allowance)
        };
        }
        engine.transport.play();
        let counts = crate::retirement::tests::allocations(|| {
            if idle {
                engine.render_idle_instruments(&mut [0.0; 16], 8, 2, 0, None, None);
            } else {
                engine.render_multitrack_segment(
                    &mut [0.0; 16],
                    render_paths::MultitrackRenderBlock {
                        pos: 0,
                        repeat_pos: 0,
                        frames: 8,
                        channels: 2,
                        loop_region: None,
                        live_input: None,
                    },
                    None,
                );
            }
        });
        assert_eq!(counts, (0, 0));
        assert!(engine.presentation_state.is_faulted(), "idle={idle}");
        assert!(!engine.transport.is_playing());
        assert_eq!(engine.presentation_state.terminals.capture_stop(), Some(0));
        assert!(engine.presentation_state.terminals.failure().is_some());
    }
}
