//! Performance rebasing closes a pending Capture boundary even when current activity is already idle.

use super::capture_lifecycle_admission_tests::{state, teardown};
use super::capture_lifecycle_transport_tests::{drain, local, prepared};
use super::*;

#[test]
fn pending_stop_rebase_preserves_first_timestamp_with_idle_or_new_active_capture() {
    for new_start in [false, true] {
        let (mut fixture, _) = prepared(7);
        let mut fillers = 0;
        while fixture
            .engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(u64::MAX))
            .is_ok()
        {
            fillers += 1;
        }
        assert_eq!(fillers, RING_BUFFER_CAPACITY);
        fixture.calls.clear();
        fixture.commands.push(local(&fixture, true)).unwrap();
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        fixture.commands.push(local(&fixture, false)).unwrap();
        if new_start {
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
        }
        drain(&mut fixture);
        let mut order = Vec::new();
        let mut times = Vec::new();
        let mut positions = Vec::new();
        for stage in ["pending", "rebased", "released"] {
            if stage == "rebased" {
                fixture
                    .commands
                    .push(EngineCommand::BeginClipPerformance)
                    .unwrap();
                drain(&mut fixture);
            }
            if stage == "released" {
                let mut observed_fillers = 0;
                while let Ok(event) = fixture.events.pop() {
                    assert!(matches!(event, EngineEvent::PlaybackPosition(u64::MAX)));
                    observed_fillers += 1;
                }
                assert_eq!(observed_fillers, fillers);
                assert_eq!(
                    crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
                    (0, 0)
                );
                let mut source = 0;
                while let Ok(event) = fixture.events.pop() {
                    match event {
                        EngineEvent::PerformanceCaptureStarted(
                            crate::events::PerformanceCaptureStartedEvent {
                                offsets,
                                effective_at_samples,
                                section_id,
                                section_position_samples,
                            },
                        ) => {
                            assert!(Arc::ptr_eq(
                                &offsets,
                                &fixture.engine.routing.capture_offsets
                            ));
                            assert_eq!((section_id, section_position_samples), (None, None));
                            order.push("start");
                            times.push(effective_at_samples);
                        }
                        EngineEvent::ClipCaptureSource {
                            track_id,
                            position,
                            effective_at_samples,
                        } => {
                            assert_eq!(track_id, fixture.tracks[source]);
                            source += 1;
                            assert_eq!(position, 25);
                            positions.push(position);
                            order.push("source");
                            times.push(effective_at_samples);
                        }
                        EngineEvent::PerformanceCaptureStopped {
                            effective_at_samples,
                        } => {
                            order.push("stop");
                            times.push(effective_at_samples);
                        }
                        EngineEvent::CompensationInvalid {
                            track_id,
                            effect_id,
                            reason,
                        } => {
                            assert_eq!(
                                (track_id, effect_id, reason),
                                (
                                    TrackId::MASTER,
                                    None,
                                    "Capture requires recovery or reload of the failed device"
                                )
                            );
                            order.push("refused");
                        }
                        EngineEvent::PerformancePosition(0) => order.push("perform-position"),
                        event => panic!("unexpected pending Capture rebase event: {event:?}"),
                    }
                }
                let mut expected = if new_start {
                    vec!["start", "source", "source"]
                } else {
                    vec![]
                };
                expected.extend(["stop", "stop", "refused", "perform-position"]);
                assert_eq!(order, expected);
                assert_eq!(positions, if new_start { vec![25, 25] } else { vec![] });
                assert_eq!(source, usize::from(new_start) * 2);
                assert!(times.iter().all(|time| *time == 25));
            }
            let observed = state(&fixture.engine);
            let pending = stage == "pending";
            let released = stage == "released";
            assert_eq!(
                observed,
                [
                    u64::from(pending && new_start),
                    u64::from(!new_start),
                    1,
                    1,
                    32,
                    if pending { 32 } else { 0 },
                    32
                ]
            );
            let terminal = fixture.engine.presentation_state.terminals.capture_stop();
            assert_eq!(terminal, (!released).then_some(25));
            let occupied = fixture.engine.presentation_state.terminals.occupied_slots();
            assert_eq!(occupied, if released { 0 } else { 2 });
            let scheduled = fixture.engine.scheduled_presentation.len();
            assert_eq!(
                scheduled,
                if released {
                    0
                } else {
                    usize::from(new_start) * 3 + if pending { 0 } else { 2 }
                }
            );
            let perform = fixture.engine.clock_domain == ClockDomain::Perform;
            assert_eq!(perform, !pending);
            let offset_refs = Arc::strong_count(&fixture.engine.routing.capture_offsets);
            assert_eq!(offset_refs, 1 + usize::from(new_start && !released));
            if pending {
                if new_start {
                    fixture.calls.assert_effect_recovered_after_failure();
                } else {
                    fixture.calls.assert_effect_failure_observed(true);
                }
            } else {
                assert!(
                    fixture.calls.observed().is_empty(),
                    "rebasing and publishing must not rescan devices"
                );
            }
            fixture.calls.clear();
        }
        teardown(fixture);
    }
}
