//! Presentation faults preserve terminal ordering, owner retention and independent compensation waits.

use super::*;
use compensation_state_test_support::{Fixture, Readiness, StartingCondition};

#[test]
fn overflowing_capture_delivers_accepted_start_then_stop_cause_and_discovering_owner() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    fixture.engine.process(&mut [], 2);
    fixture.drain_events();
    let capture_start = fixture.engine.heard_capture_position();
    let capture_stop = fixture.engine.compensation_callback_heard_start;
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    fixture.drain_command(EngineCommand::StartPerformanceCapture);
    let accepted_before_filler = fixture.engine.scheduled_presentation.len();
    assert_eq!(accepted_before_filler, 3);
    assert!(matches!(
        fixture.engine.scheduled_presentation[0].event,
        EngineEvent::PerformanceCaptureStarted { .. }
    ));
    let accepted_sources: Vec<_> = fixture.engine.scheduled_presentation[1..]
        .iter()
        .map(|pending| match pending.event {
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => (track_id, position, effective_at_samples),
            ref event => panic!("unexpected accepted Capture source: {event:?}"),
        })
        .collect();
    for (source, track) in accepted_sources.iter().zip(fixture.tracks) {
        assert_eq!(source.0, track);
        assert_eq!(source.2, capture_start);
    }
    let remaining = fixture.engine.scheduled_presentation.capacity() - accepted_before_filler;
    for _ in 0..remaining {
        fixture.engine.present_event_at(
            EngineEvent::PlaybackStarted,
            fixture.engine.output_position + 521,
        );
    }
    let fault_before_owner = fixture.engine.presentation_fault;
    assert!(!fault_before_owner);
    assert!(fixture.engine.presentation_overflow_owner.is_none());
    assert_eq!(
        fixture.engine.scheduled_presentation.len(),
        fixture.engine.scheduled_presentation.capacity()
    );
    let offsets: Arc<[(TrackId, u32)]> = vec![(fixture.tracks[0], 17)].into();
    let discovering = EngineEvent::SectionCaptureSource {
        section_id: SectionId::new(),
        effective_at_samples: 123,
        section_position_samples: 45,
        refreshed: false,
        offsets: Arc::clone(&offsets),
    };
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .present_event_at(discovering, fixture.engine.output_position + 521)),
        (0, 0)
    );
    assert!(fixture.engine.presentation_fault);
    assert!(fixture.engine.compensation_valid);
    assert!(!fixture.engine.capture_active);
    assert!(!fixture.engine.transport.is_playing());
    assert_eq!(fixture.engine.pending_capture_stop, Some(capture_stop));
    assert!(fixture.engine.pending_playback_stop);
    assert_eq!(
        fixture.engine.pending_compensation_failure.unwrap().2,
        "Audio presentation event history is full"
    );
    assert!(matches!(
        fixture.engine.presentation_overflow_owner,
        Some(EngineEvent::CaptureTimingRetired(_))
    ), "accepted_before_filler={accepted_before_filler}, fault_before_owner={fault_before_owner}, overflow_owner={:?}, owner_domain={:?}, queued={}, capacity={}",
        fixture.engine.presentation_overflow_owner,
        fixture.engine.presentation_overflow_owner.as_ref().map(EngineEvent::time_domain),
        fixture.engine.scheduled_presentation.len(),
        fixture.engine.scheduled_presentation.capacity());
    assert_eq!(Arc::strong_count(&offsets), 2);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.fail_presentation()),
        (0, 0)
    );
    while fixture.events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
        (0, 0)
    );
    let events: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok()).collect();
    let mut source_index = 0;
    let order: Vec<_> = events
        .iter()
        .map(|event| match event {
            EngineEvent::PerformanceCaptureStarted {
                effective_at_samples,
                ..
            } => {
                assert_eq!(*effective_at_samples, capture_start);
                "capture-start"
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                assert_eq!(
                    (*track_id, *position, *effective_at_samples),
                    accepted_sources[source_index]
                );
                source_index += 1;
                "capture-source"
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                assert_eq!(*effective_at_samples, capture_stop);
                "capture-stop"
            }
            EngineEvent::CompensationInvalid { reason, .. } => {
                assert_eq!(*reason, "Audio presentation event history is full");
                "cause"
            }
            EngineEvent::PlaybackStopped => "playback-stop",
            EngineEvent::CaptureTimingRetired(owner) => {
                assert!(Arc::ptr_eq(owner, &offsets));
                "owner"
            }
            _ => panic!("unexpected terminal event: {event:?}"),
        })
        .collect();
    assert_eq!(
        order,
        [
            "capture-start",
            "capture-source",
            "capture-source",
            "capture-stop",
            "cause",
            "playback-stop",
            "owner"
        ]
    );
    assert_eq!(source_index, 2);
    assert!(fixture.engine.pending_capture_stop.is_none());
    assert!(fixture.engine.pending_compensation_failure.is_none());
    assert!(!fixture.engine.pending_playback_stop);
    assert!(fixture.engine.presentation_overflow_owner.is_none());
    assert!(fixture.engine.scheduled_presentation.is_empty());
    drop(events);
    assert_eq!(Arc::strong_count(&offsets), 1);
    assert!(fixture.drain_events().is_empty());
    let plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
    fixture.drain_command(EngineCommand::Play);
    assert!(!fixture.engine.presentation_fault);
    assert!(!fixture.engine.project_processing_muted());
    assert!(fixture.engine.transport.is_playing());
    assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), plan);
}

#[test]
fn play_clears_presentation_exhaustion_without_repairing_an_independent_compensation_wait() {
    for start in [
        StartingCondition::AwaitingPlan,
        StartingCondition::DeviceAway,
        StartingCondition::InvalidPlan,
    ] {
        let mut fixture = Fixture::new(start);
        let before = Readiness::observe(&fixture.engine);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.fail_presentation()),
            (0, 0)
        );
        assert_eq!(
            Readiness::observe(&fixture.engine),
            Readiness {
                presentation_fault: true,
                playing: false,
                ..before
            }
        );
        fixture.drain_events();
        fixture.drain_command(EngineCommand::Play);
        assert_eq!(
            Readiness::observe(&fixture.engine),
            Readiness {
                playing: true,
                ..before
            }
        );
    }
}
