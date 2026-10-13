//! Delayed public state and immediate recording feeds keep their distinct delivery order.

use super::*;
use crate::events::SourceRecordingPosition;
use crate::playback_source::PreparedSectionPlaybackSource;

#[test]
fn in_segment_source_feed_does_not_wait_for_a_future_heard_deadline() {
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.present_event_after(EngineEvent::PlaybackStarted, 137);
    engine.rendered_callback_frames = 200;
    engine.command_external_note_on(TrackId::new(), 60, 100);
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::SourceNoteInput { .. })
    ));
    assert!(events.pop().is_err());
    engine.rendered_callback_frames = 0;
    engine.output_position = 137;
    engine.flush_presentation();
    assert!(matches!(events.pop(), Ok(EngineEvent::PlaybackStarted)));
}

#[test]
fn in_segment_repeat_source_is_immediate_without_advancing_its_heard_copy() {
    use vibez_core::perform::NoteRepeatRate;
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.present_event_after(EngineEvent::PlaybackStarted, 137);
    let track_id = TrackId::new();
    let rate = NoteRepeatRate::Sixteenth;
    let source = EngineEvent::SourceNoteRepeated {
        track_id,
        pitch: 60,
        velocity: 100,
        rate,
        position: SourceRecordingPosition::default(),
    };
    let heard = EngineEvent::NoteRepeated {
        track_id,
        pitch: 60,
        velocity: 100,
        rate,
        effective_at_samples: 0,
        canonical_at_samples: 0,
        section_id: None,
        section_position_samples: None,
        canonical_section_position_samples: None,
    };
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            assert!(presentation_queue::emit_repeated(
                source,
                heard,
                &mut engine.event_tx,
                &mut engine.scheduled_presentation,
                200,
                337,
                0
            ));
        }),
        (0, 0)
    );
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::SourceNoteRepeated { .. })
    ));
    assert!(events.pop().is_err());
    engine.output_position = 137;
    engine.flush_presentation();
    assert!(matches!(events.pop(), Ok(EngineEvent::PlaybackStarted)));
    assert!(events.pop().is_err());
    engine.output_position = 337;
    engine.flush_presentation();
    assert!(matches!(events.pop(), Ok(EngineEvent::NoteRepeated { .. })));
}

#[test]
fn due_capture_notes_precede_closure_and_continuing_pad_feedback_survives() {
    for closure in 0..3 {
        let (mut engine, _, mut events) = AudioEngine::new();
        let track_id = TrackId::new();
        let note = |pitch| EngineEvent::InstrumentNoteInput {
            track_id,
            pitch,
            velocity: 100,
            on: true,
            effective_at_samples: pitch as u64,
            section_id: None,
            section_position_samples: None,
        };
        engine.present_event_after(note(60), 137);
        engine.present_event_after(note(61), 138);
        engine.output_position = 137;
        engine.compensation_callback_heard_start = 137;
        engine.transport.seek(137);
        while engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(0))
            .is_ok()
        {}
        assert_eq!(
            crate::retirement::tests::allocations(|| match closure {
                0 => engine.command_stop_performance_capture(),
                1 => engine.close_capture_for_device_failure(),
                _ => engine.command_stop(),
            }),
            (0, 0)
        );
        while events.pop().is_ok() {}
        engine.flush_presentation();
        let received: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        let note = received
            .iter()
            .position(|event| matches!(event, EngineEvent::InstrumentNoteInput { pitch: 60, .. }));
        let stop = received
            .iter()
            .position(|event| matches!(event, EngineEvent::PerformanceCaptureStopped { .. }));
        assert!(
            note.is_some() && stop.is_some() && note < stop,
            "closure {closure}"
        );
        engine.output_position = 138;
        engine.flush_presentation();
        let future_feedback = std::iter::from_fn(|| events.pop().ok())
            .any(|event| matches!(event, EngineEvent::InstrumentNoteInput { pitch: 61, .. }));
        assert_eq!(future_feedback, closure != 2);
    }
}

#[test]
fn accepted_due_capture_start_reaches_the_ui_before_its_note_and_closure() {
    for closure in 0..3 {
        let (mut engine, _, mut events) = AudioEngine::new();
        let track_id = TrackId::new();
        let offsets: Arc<[(TrackId, u32)]> = vec![(track_id, 384)].into();
        while engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(0))
            .is_ok()
        {}
        engine.present_event(EngineEvent::PerformanceCaptureStarted {
            effective_at_samples: 0,
            section_id: None,
            section_position_samples: None,
            offsets: Arc::clone(&offsets),
        });
        engine.present_event_after(
            EngineEvent::InstrumentNoteInput {
                track_id,
                pitch: 60,
                velocity: 100,
                on: true,
                effective_at_samples: 100,
                section_id: None,
                section_position_samples: None,
            },
            100,
        );
        engine.output_position = 137;
        engine.compensation_callback_heard_start = 137;
        engine.transport.seek(137);
        assert_eq!(
            crate::retirement::tests::allocations(|| match closure {
                0 => engine.command_stop_performance_capture(),
                1 => engine.close_capture_for_device_failure(),
                _ => engine.command_stop(),
            }),
            (0, 0)
        );
        while events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.flush_presentation()),
            (0, 0)
        );
        let received: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        let start = received
            .iter()
            .position(|event| matches!(event, EngineEvent::PerformanceCaptureStarted { .. }));
        let note = received
            .iter()
            .position(|event| matches!(event, EngineEvent::InstrumentNoteInput { .. }));
        let stop = received
            .iter()
            .position(|event| matches!(event, EngineEvent::PerformanceCaptureStopped { .. }));
        assert!(
            start.is_some() && note.is_some() && stop.is_some() && start < note && note < stop,
            "closure {closure}"
        );
        drop(received);
        assert_eq!(Arc::strong_count(&offsets), 1);
    }
}

#[test]
fn state_queued_after_completed_callback_waits_only_its_real_latency() {
    use crate::test_support::DelayProbe;
    use vibez_core::{id::EffectId, routing::*};

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: TrackId::MASTER,
            effect_id: effect,
            effect: Box::new(DelayProbe::new(137)),
            position: None,
        })
        .unwrap();
    let channels = [RoutingChannel {
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
    let reports = [(
        RoutingNode {
            channel: TrackId::MASTER,
            stage: NodeStage::Effect(effect),
        },
        137,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    engine.process(&mut [0.0; 128], 2);
    while events.pop().is_ok() {}
    engine.present_event_after(EngineEvent::PlaybackStarted, engine.mix_latency());
    for frames in [64, 64, 8] {
        let mut output = [0.0; 128];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output[..frames * 2], 2)),
            (0, 0)
        );
        assert!(!std::iter::from_fn(|| events.pop().ok())
            .any(|event| matches!(event, EngineEvent::PlaybackStarted)));
    }
    assert_eq!(engine.output_position, 200);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 2], 2)),
        (0, 0)
    );
    assert!(std::iter::from_fn(|| events.pop().ok())
        .any(|event| matches!(event, EngineEvent::PlaybackStarted)));
}

#[test]
fn newer_immediate_mute_cannot_overtake_a_due_delayed_mute() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    engine.present_event_after(
        EngineEvent::TrackMuteChanged {
            track_id,
            muted: true,
            effective_at_samples: 137,
        },
        137,
    );
    engine.output_position = 137;
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    events.pop().unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.present_event(EngineEvent::TrackMuteChanged {
                track_id,
                muted: false,
                effective_at_samples: 137,
            });
        }),
        (0, 0)
    );
    let mut muted = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::TrackMuteChanged { muted: value, .. } = event {
            muted.push(value);
        }
    }
    engine.flush_presentation();
    while let Ok(event) = events.pop() {
        if let EngineEvent::TrackMuteChanged { muted: value, .. } = event {
            muted.push(value);
        }
    }
    assert_eq!(muted, [true, false]);
}

#[test]
fn source_recording_feed_does_not_wait_for_future_heard_state() {
    let (mut engine, _, mut events) = AudioEngine::new();
    engine.present_event_after(EngineEvent::PlaybackStarted, 521);
    let position = SourceRecordingPosition {
        effective_at_samples: 0,
        ..Default::default()
    };
    engine.present_event(EngineEvent::SourceNoteInput {
        track_id: TrackId::new(),
        pitch: 60,
        velocity: 100,
        on: true,
        position,
    });
    assert!(
        matches!(events.pop(), Ok(EngineEvent::SourceNoteInput { position: actual, .. }) if actual == position)
    );
    assert!(events.pop().is_err());
    engine.output_position = 521;
    engine.flush_presentation();
    assert!(matches!(events.pop(), Ok(EngineEvent::PlaybackStarted)));
}

#[test]
fn capture_only_cancellation_retains_offsets_and_public_section_owner() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let section_id = SectionId::new();
    let offsets: Arc<[(TrackId, u32)]> = vec![(TrackId::new(), 384)].into();
    let prepared = Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        8.0,
        true,
        vec![],
    ));
    let original = &*prepared as *const PreparedSectionPlaybackSource;
    engine.present_event_after(
        EngineEvent::SectionTransitioned {
            section_id,
            effective_at_samples: 521,
            retired: prepared,
        },
        521,
    );
    let track_id = TrackId::new();
    engine.present_event_after(
        EngineEvent::TrackMuteChanged {
            track_id,
            muted: true,
            effective_at_samples: 64,
        },
        64,
    );
    engine.present_event_after(
        EngineEvent::SectionCaptureSource {
            section_id,
            effective_at_samples: 521,
            section_position_samples: 0,
            refreshed: false,
            offsets: Arc::clone(&offsets),
        },
        137,
    );
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    let position = SourceRecordingPosition {
        effective_at_samples: 0,
        ..Default::default()
    };
    engine.present_event(EngineEvent::SourceNoteInput {
        track_id: TrackId::new(),
        pitch: 60,
        velocity: 100,
        on: true,
        position,
    });
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.close_capture_for_device_failure()),
        (0, 0)
    );
    assert_eq!(Arc::strong_count(&offsets), 2);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    let mut source_seen = false;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::SourceNoteInput {
                position: actual, ..
            } => {
                assert_eq!(actual, position);
                source_seen = true;
            }
            EngineEvent::TrackMuteChanged { .. } => panic!("future public mute arrived early"),
            EngineEvent::CaptureTimingRetired(_) => panic!("future owner retirement arrived early"),
            _ => {}
        }
    }
    assert!(source_seen);
    engine.output_position = 64;
    engine.flush_presentation();
    assert!(
        matches!(events.pop(), Ok(EngineEvent::TrackMuteChanged { track_id: actual, muted: true, .. }) if actual == track_id)
    );
    assert!(events.pop().is_err());
    engine.output_position = 137;
    engine.flush_presentation();
    let retired_offsets = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::CaptureTimingRetired(offsets) => Some(offsets),
        _ => None,
    });
    assert!(retired_offsets.is_some());
    drop(retired_offsets);
    assert_eq!(Arc::strong_count(&offsets), 1);
    engine.output_position = 521;
    engine.flush_presentation();
    let retired = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionTransitioned { retired, .. } => Some(retired),
        _ => None,
    });
    assert_eq!(
        &*retired.unwrap() as *const PreparedSectionPlaybackSource,
        original
    );
}
