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
    let fault_before_owner = fixture.engine.presentation_state.is_faulted();
    assert!(!fault_before_owner);
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_none());
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
    assert!(fixture.engine.presentation_state.is_faulted());
    assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
    assert!(!fixture.engine.capture_active);
    assert!(!fixture.engine.transport.is_playing());
    assert_eq!(
        fixture.engine.presentation_state.terminals.capture_stop(),
        Some(capture_stop)
    );
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .playback_stop_pending());
    assert_eq!(
        fixture
            .engine
            .presentation_state
            .terminals
            .failure()
            .unwrap()
            .2,
        "Audio presentation event history is full"
    );
    assert!(matches!(
        fixture.engine.presentation_state.terminals.overflow_owner(),
        Some(EngineEvent::CaptureTimingRetired(_))
    ), "accepted_before_filler={accepted_before_filler}, fault_before_owner={fault_before_owner}, overflow_owner={:?}, owner_domain={:?}, queued={}, capacity={}",
        fixture.engine.presentation_state.terminals.overflow_owner(),
        fixture.engine.presentation_state.terminals.overflow_owner().map(EngineEvent::time_domain),
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
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .capture_stop()
        .is_none());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .failure()
        .is_none());
    assert!(!fixture
        .engine
        .presentation_state
        .terminals
        .playback_stop_pending());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_none());
    assert!(fixture.engine.scheduled_presentation.is_empty());
    drop(events);
    assert_eq!(Arc::strong_count(&offsets), 1);
    assert!(fixture.drain_events().is_empty());
    let plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
    fixture.drain_command(EngineCommand::Play);
    assert!(!fixture.engine.presentation_state.is_faulted());
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

#[test]
fn play_with_full_ui_ring_retains_capture_terminals_and_owner_but_cancels_inline_playback_stop() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    fixture.engine.process(&mut [], 2);
    fixture.drain_events();
    let plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
    let level = fixture.engine.output_gate.last_frame();
    let capture_start = fixture.engine.heard_capture_position();
    let capture_stop = fixture.engine.compensation_callback_heard_start;
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    fixture.drain_command(EngineCommand::StartPerformanceCapture);
    assert_eq!(fixture.engine.scheduled_presentation.len(), 3);
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
    let remaining = fixture.engine.scheduled_presentation.capacity()
        - fixture.engine.scheduled_presentation.len();
    for _ in 0..remaining {
        fixture.engine.present_event_at(
            EngineEvent::PlaybackStarted,
            fixture.engine.output_position + 521,
        );
    }
    assert!(!fixture.engine.presentation_state.is_faulted());
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
    assert!(fixture.engine.presentation_state.is_faulted());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .playback_stop_pending());
    let mut loss = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.process(&mut loss, 2)),
        (0, 0)
    );
    assert_eq!(fixture.engine.event_tx.slots(), 0);
    fixture.drain_command(EngineCommand::Play);
    assert!(!fixture.engine.presentation_state.is_faulted());
    assert!(!fixture
        .engine
        .presentation_state
        .terminals
        .playback_stop_pending());
    assert!(fixture.engine.transport.is_playing());
    assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
    assert!(!fixture.engine.project_processing_muted());
    assert!(!fixture.engine.capture_active);
    assert_eq!(
        fixture.engine.presentation_state.terminals.capture_stop(),
        Some(capture_stop)
    );
    assert_eq!(
        fixture.engine.presentation_state.terminals.failure(),
        Some((
            TrackId::MASTER,
            None,
            "Audio presentation event history is full"
        ))
    );
    assert!(matches!(
        fixture.engine.presentation_state.terminals.overflow_owner(),
        Some(EngineEvent::CaptureTimingRetired(owner)) if Arc::ptr_eq(owner, &offsets)
    ));
    assert_eq!(Arc::strong_count(&offsets), 2);
    assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), plan);
    let mut resumed = [0.0; 256];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.process(&mut resumed, 2)),
        (0, 0)
    );
    for (frame, pair) in loss.as_chunks::<2>().0.iter().enumerate() {
        for channel in 0..2 {
            let expected = level[channel] * (64 - frame) as f32 / 64.0;
            assert!((pair[channel] - expected).abs() < 1e-7);
        }
    }
    for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
        for channel in 0..2 {
            let expected = level[channel] * frame.min(64) as f32 / 64.0;
            assert!((pair[channel] - expected).abs() < 1e-7);
        }
    }
    assert_eq!(fixture.engine.event_tx.slots(), 0);
    let events = fixture.drain_events();
    let mut source_index = 0;
    let mut order = Vec::new();
    for event in &events {
        match event {
            EngineEvent::PlaybackPosition(0) => {}
            EngineEvent::PerformanceCaptureStarted {
                effective_at_samples,
                ..
            } => {
                assert_eq!(*effective_at_samples, capture_start);
                order.push("capture-start");
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
                order.push("capture-source");
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                assert_eq!(*effective_at_samples, capture_stop);
                order.push("capture-stop");
            }
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!(
                    (*track_id, *effect_id, *reason),
                    (
                        TrackId::MASTER,
                        None,
                        "Audio presentation event history is full"
                    )
                );
                order.push("cause");
            }
            EngineEvent::CaptureTimingRetired(owner) => {
                assert!(Arc::ptr_eq(owner, &offsets));
                order.push("owner");
            }
            EngineEvent::PlaybackStarted => order.push("playback-start"),
            _ => panic!("unexpected event after Play with retained terminals: {event:?}"),
        }
    }
    assert_eq!(
        order,
        [
            "capture-start",
            "capture-source",
            "capture-source",
            "capture-stop",
            "cause",
            "owner",
            "playback-start"
        ]
    );
    assert_eq!(source_index, 2);
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .capture_stop()
        .is_none());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .failure()
        .is_none());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .overflow_owner()
        .is_none());
    assert!(!fixture
        .engine
        .presentation_state
        .terminals
        .playback_stop_pending());
    assert!(fixture.engine.scheduled_presentation.is_empty());
    drop(events);
    assert_eq!(Arc::strong_count(&offsets), 1);
    assert!(fixture.drain_events().is_empty());
}

#[test]
fn capture_terminals_keep_first_writer_after_public_backward_seek() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    fixture.engine.process(&mut [], 2);
    fixture.drain_events();
    let first_boundary = fixture.engine.compensation_callback_heard_start;
    assert!(first_boundary > 0);
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    for _ in 0..fixture.engine.scheduled_presentation.capacity() {
        fixture.engine.present_event_at(
            EngineEvent::PlaybackStarted,
            fixture.engine.output_position + 521,
        );
    }
    assert!(!fixture.engine.presentation_state.is_faulted());
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.present_event_at(
            EngineEvent::PlaybackStarted,
            fixture.engine.output_position + 521,
        )),
        (0, 0)
    );
    assert!(fixture.engine.presentation_state.is_faulted());
    assert_eq!(
        fixture.engine.presentation_state.terminals.capture_stop(),
        Some(first_boundary)
    );
    fixture.drain_command(EngineCommand::Seek(0));
    let later_boundary = fixture.engine.heard_capture_position();
    assert_eq!(later_boundary, 0);
    assert!(later_boundary < first_boundary);
    fixture.drain_command(EngineCommand::StopPerformanceCapture);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.report_compensation_failure(
            fixture.tracks[0],
            Some(fixture.effect),
            "Later failure must not replace the retained cause",
        )),
        (0, 0)
    );
    assert_eq!(
        fixture.engine.presentation_state.terminals.capture_stop(),
        Some(first_boundary)
    );
    assert_eq!(
        fixture.engine.presentation_state.terminals.failure(),
        Some((
            TrackId::MASTER,
            None,
            "Audio presentation event history is full"
        ))
    );
    assert_eq!(fixture.engine.event_tx.slots(), 0);
    let events = fixture.drain_events();
    let mut order = Vec::new();
    for event in &events {
        match event {
            EngineEvent::PlaybackPosition(0) | EngineEvent::PresentationCancelled => {}
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                assert_eq!(*effective_at_samples, first_boundary);
                order.push("capture-stop");
            }
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!(*track_id, TrackId::MASTER);
                assert_eq!(*effect_id, None);
                assert_eq!(*reason, "Audio presentation event history is full");
                order.push("cause");
            }
            EngineEvent::PlaybackStopped => order.push("playback-stop"),
            event => panic!("unexpected retained terminal: {event:?}"),
        }
    }
    assert_eq!(order, ["capture-stop", "cause", "playback-stop"]);
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .capture_stop()
        .is_none());
    assert!(fixture
        .engine
        .presentation_state
        .terminals
        .failure()
        .is_none());
    assert!(fixture.drain_events().is_empty());
}

#[test]
fn partial_terminal_publication_preserves_each_full_boundary_and_exact_overflow_owner() {
    for credits in 0..=4 {
        let mut fixture = Fixture::new(StartingCondition::Running);
        fixture.engine.process(&mut [], 2);
        fixture.drain_events();
        let capture_stop = fixture.engine.compensation_callback_heard_start;
        let plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
        while fixture
            .engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(0))
            .is_ok()
        {}
        for _ in 0..fixture.engine.scheduled_presentation.capacity() {
            fixture.engine.present_event_at(
                EngineEvent::PlaybackStarted,
                fixture.engine.output_position + 521,
            );
        }
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
                .present_event_at(discovering, fixture.engine.output_position + 521,)),
            (0, 0)
        );
        assert_eq!(
            fixture.engine.presentation_state.terminals.occupied_slots(),
            4
        );
        assert!(fixture.engine.presentation_state.is_faulted());
        assert_eq!(Arc::strong_count(&offsets), 2);
        for _ in 0..credits {
            assert!(matches!(
                fixture.events.pop().unwrap(),
                EngineEvent::PlaybackPosition(0)
            ));
        }
        assert_eq!(fixture.engine.event_tx.slots(), credits);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
            (0, 0)
        );
        assert_eq!(fixture.engine.event_tx.slots(), 0);
        assert_eq!(
            fixture.engine.presentation_state.terminals.occupied_slots(),
            4 - credits
        );
        assert_eq!(
            fixture.engine.presentation_state.terminals.capture_stop(),
            (credits == 0).then_some(capture_stop)
        );
        assert_eq!(
            fixture
                .engine
                .presentation_state
                .terminals
                .failure()
                .is_some(),
            credits < 2
        );
        assert_eq!(
            fixture
                .engine
                .presentation_state
                .terminals
                .playback_stop_pending(),
            credits < 3
        );
        match fixture.engine.presentation_state.terminals.overflow_owner() {
            Some(EngineEvent::CaptureTimingRetired(owner)) => {
                assert!(credits < 4);
                assert!(Arc::ptr_eq(owner, &offsets));
            }
            None => assert_eq!(credits, 4),
            event => panic!("unexpected retained overflow owner: {event:?}"),
        }
        assert_eq!(Arc::strong_count(&offsets), 2);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
            (0, 0)
        );
        assert_eq!(
            fixture.engine.presentation_state.terminals.occupied_slots(),
            4 - credits
        );
        assert_eq!(Arc::strong_count(&offsets), 2);
        let mut delivered: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok())
            .filter(|event| !matches!(event, EngineEvent::PlaybackPosition(0)))
            .collect();
        assert_eq!(delivered.len(), credits);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
            (0, 0)
        );
        delivered.extend(std::iter::from_fn(|| fixture.events.pop().ok()));
        let order: Vec<_> = delivered
            .iter()
            .map(|event| match event {
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                } => {
                    assert_eq!(*effective_at_samples, capture_stop);
                    "capture-stop"
                }
                EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id,
                    reason,
                } => {
                    assert_eq!(
                        (*track_id, *effect_id, *reason),
                        (
                            TrackId::MASTER,
                            None,
                            "Audio presentation event history is full"
                        )
                    );
                    "cause"
                }
                EngineEvent::PlaybackStopped => "playback-stop",
                EngineEvent::CaptureTimingRetired(owner) => {
                    assert!(Arc::ptr_eq(owner, &offsets));
                    "owner"
                }
                event => panic!("unexpected partial terminal: {event:?}"),
            })
            .collect();
        assert_eq!(order, ["capture-stop", "cause", "playback-stop", "owner"]);
        assert_eq!(
            fixture.engine.presentation_state.terminals.occupied_slots(),
            0
        );
        assert!(fixture.engine.scheduled_presentation.is_empty());
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), plan);
        assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
        assert!(!fixture.engine.transport.is_playing());
        assert!(!fixture.engine.capture_active);
        assert!(fixture.engine.presentation_state.is_faulted());
        drop(delivered);
        assert_eq!(Arc::strong_count(&offsets), 1);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
            (0, 0)
        );
        assert!(fixture.events.pop().is_err());
    }
}
