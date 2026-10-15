//! Retained notifications and accepted Capture activity have independent lifetimes under UI pressure.

use super::capture_lifecycle_admission_tests::{discard_setup, fixture, state, teardown, Fixture};
use super::*;

fn local(fixture: &Fixture, value: bool) -> EngineCommand {
    EngineCommand::SetEffectParam {
        track_id: fixture.tracks[0],
        effect_id: fixture.effect,
        param_index: 0,
        value: f32::from(u8::from(value)),
    }
}

fn zero(fixture: &mut Fixture) {
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
}

fn drain(fixture: &mut Fixture) {
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
}

fn full(fixture: &mut Fixture) {
    let mut count = 0;
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(u64::MAX))
        .is_ok()
    {
        count += 1;
    }
    assert_eq!(count, RING_BUFFER_CAPACITY);
}

#[derive(Default)]
struct Delivered {
    order: Vec<&'static str>,
    times: Vec<u64>,
    sources: usize,
    fillers: usize,
}

fn collect(
    fixture: &mut Fixture,
    delivered: &mut Delivered,
    boundary: u64,
    overflow: Option<&Arc<[(TrackId, u32)]>>,
) {
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PlaybackPosition(u64::MAX) => delivered.fillers += 1,
            EngineEvent::PerformanceCaptureStarted {
                offsets,
                effective_at_samples,
                section_id,
                section_position_samples,
            } => {
                assert!(Arc::ptr_eq(
                    &offsets,
                    &fixture.engine.routing.capture_offsets
                ));
                assert_eq!((section_id, section_position_samples), (None, None));
                delivered.order.push("start");
                delivered.times.push(effective_at_samples);
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                assert_eq!(track_id, fixture.tracks[delivered.sources]);
                assert_eq!(position, 0);
                delivered.sources += 1;
                delivered.order.push("source");
                delivered.times.push(effective_at_samples);
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                delivered.order.push("stop");
                delivered.times.push(effective_at_samples);
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
                        "Audio presentation event history is full"
                    )
                );
                delivered.order.push("cause");
            }
            EngineEvent::CaptureTimingRetired(offsets) => {
                assert!(Arc::ptr_eq(&offsets, overflow.unwrap()));
                delivered.order.push("owner");
            }
            EngineEvent::PlaybackStarted => delivered.order.push("playback-start"),
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture pressure event: {event:?}"),
        }
    }
    assert!(delivered.times.iter().all(|time| *time == boundary));
}

struct Expected {
    state: [u64; 7],
    scheduled: usize,
    terminal: Option<u64>,
    terminals: usize,
    fault: bool,
    offset_refs: usize,
    overflow_refs: usize,
    full: bool,
}

fn record(
    fixture: &mut Fixture,
    case: &str,
    expected: Expected,
    overflow: Option<&Arc<[(TrackId, u32)]>>,
) {
    let observed_state = state(&fixture.engine);
    assert_eq!(observed_state, expected.state, "{case}");
    let scheduled = fixture.engine.scheduled_presentation.len();
    assert_eq!(scheduled, expected.scheduled, "{case}");
    let terminal = fixture.engine.presentation_state.terminals.capture_stop();
    assert_eq!(terminal, expected.terminal, "{case}");
    let terminals = fixture.engine.presentation_state.terminals.occupied_slots();
    assert_eq!(terminals, expected.terminals, "{case}");
    let fault = fixture.engine.presentation_state.is_faulted();
    assert_eq!(fault, expected.fault, "{case}");
    let offset_refs = Arc::strong_count(&fixture.engine.routing.capture_offsets);
    assert_eq!(offset_refs, expected.offset_refs, "{case}");
    let overflow_refs = overflow.map_or(0, Arc::strong_count);
    assert_eq!(overflow_refs, expected.overflow_refs, "{case}");
    let full = fixture.engine.event_tx.slots() == 0;
    assert_eq!(full, expected.full, "{case}");
    let _calls = fixture.calls.observed();
    fixture.calls.clear();
}

#[test]
fn old_local_stop_and_new_activity_survive_zero_one_two_and_remaining_ui_credits() {
    for explicit_stop in [false, true] {
        let mut fixture = fixture();
        let mut audio = [0.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut audio, 2))),
            (0, 0)
        );
        for frame in audio.as_chunks::<2>().0 {
            assert_eq!(*frame, [0.25, 0.0]);
        }
        discard_setup(&mut fixture);
        full(&mut fixture);
        fixture.commands.push(local(&fixture, true)).unwrap();
        zero(&mut fixture);
        assert_eq!(
            fixture.engine.presentation_state.terminals.capture_stop(),
            Some(8)
        );
        fixture.commands.push(local(&fixture, false)).unwrap();
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        fixture.calls.clear();
        drain(&mut fixture);
        let label = if explicit_stop {
            "public-stop"
        } else {
            "active"
        };
        record(
            &mut fixture,
            &format!("{label}-accepted"),
            Expected {
                state: [1, 1, 1, 1, 8, 8, 8],
                scheduled: 3,
                terminal: Some(8),
                terminals: 1,
                fault: false,
                offset_refs: 2,
                overflow_refs: 0,
                full: true,
            },
            None,
        );
        if explicit_stop {
            fixture
                .commands
                .push(EngineCommand::StopPerformanceCapture)
                .unwrap();
        }
        let count = 3 + usize::from(explicit_stop);
        let active = u64::from(!explicit_stop);
        for (stage, credits, published) in [("zero", 0, 0), ("one", 1, 1), ("two", 2, 3)] {
            for _ in 0..credits {
                assert!(matches!(
                    fixture.events.pop().unwrap(),
                    EngineEvent::PlaybackPosition(u64::MAX)
                ));
            }
            assert_eq!(fixture.engine.event_tx.slots(), credits);
            zero(&mut fixture);
            record(
                &mut fixture,
                &format!("{label}-{stage}"),
                Expected {
                    state: [active, 0, 1, 1, 8, 8, 8],
                    scheduled: count - published,
                    terminal: Some(8),
                    terminals: 1,
                    fault: false,
                    offset_refs: 2,
                    overflow_refs: 0,
                    full: true,
                },
                None,
            );
        }
        let mut delivered = Delivered::default();
        collect(&mut fixture, &mut delivered, 8, None);
        assert_eq!(delivered.fillers, RING_BUFFER_CAPACITY - 3);
        assert_eq!(delivered.order, ["start", "source", "source"]);
        zero(&mut fixture);
        collect(&mut fixture, &mut delivered, 8, None);
        let mut order = vec!["start", "source", "source"];
        if explicit_stop {
            order.push("stop");
        }
        order.push("stop");
        assert_eq!(delivered.order, order);
        assert_eq!(delivered.sources, 2);
        record(
            &mut fixture,
            &format!("{label}-released"),
            Expected {
                state: [active, 0, 1, 1, 8, 8, 8],
                scheduled: 0,
                terminal: None,
                terminals: 0,
                fault: false,
                offset_refs: 1,
                overflow_refs: 0,
                full: false,
            },
            None,
        );
        teardown(fixture);
    }
}

#[test]
fn repeated_fault_and_play_keep_the_closed_episode_until_an_admitted_clear_validation() {
    let mut fixture = fixture();
    fixture.commands.push(local(&fixture, true)).unwrap();
    zero(&mut fixture);
    discard_setup(&mut fixture);
    fixture.commands.push(local(&fixture, false)).unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    discard_setup(&mut fixture);
    assert_eq!(state(&fixture.engine), [1, 1, 1, 1, 0, 0, 0]);
    full(&mut fixture);
    for _ in 0..fixture.engine.scheduled_presentation.capacity() {
        fixture
            .engine
            .present_event_after(EngineEvent::PlaybackStarted, 521);
    }
    let overflow: Arc<[(TrackId, u32)]> = vec![(fixture.tracks[0], 17)].into();
    let discovering = EngineEvent::SectionCaptureSource {
        section_id: SectionId::new(),
        effective_at_samples: 521,
        section_position_samples: 0,
        refreshed: false,
        offsets: Arc::clone(&overflow),
    };
    fixture.calls.clear();
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .present_event_after(discovering, 521)),
        (0, 0)
    );
    for stage in ["fault", "repeated-fault"] {
        if stage == "repeated-fault" {
            assert_eq!(
                crate::retirement::tests::allocations(|| fixture.engine.fail_presentation()),
                (0, 0)
            );
        }
        record(
            &mut fixture,
            stage,
            Expected {
                state: [0, 1, 1, 0, 0, 0, 0],
                scheduled: 0,
                terminal: Some(0),
                terminals: 4,
                fault: true,
                offset_refs: 1,
                overflow_refs: 2,
                full: true,
            },
            Some(&overflow),
        );
    }
    fixture.commands.push(EngineCommand::Play).unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    record(
        &mut fixture,
        "play-new-start",
        Expected {
            state: [1, 1, 1, 1, 0, 0, 0],
            scheduled: 4,
            terminal: Some(0),
            terminals: 3,
            fault: false,
            offset_refs: 2,
            overflow_refs: 2,
            full: true,
        },
        Some(&overflow),
    );
    zero(&mut fixture);
    record(
        &mut fixture,
        "play-clear-validation",
        Expected {
            state: [1, 0, 1, 1, 0, 0, 0],
            scheduled: 4,
            terminal: Some(0),
            terminals: 3,
            fault: false,
            offset_refs: 2,
            overflow_refs: 2,
            full: true,
        },
        Some(&overflow),
    );
    let mut delivered = Delivered::default();
    collect(&mut fixture, &mut delivered, 0, Some(&overflow));
    assert_eq!(delivered.fillers, RING_BUFFER_CAPACITY);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.flush_presentation()),
        (0, 0)
    );
    collect(&mut fixture, &mut delivered, 0, Some(&overflow));
    assert_eq!(
        delivered.order,
        [
            "start",
            "source",
            "source",
            "stop",
            "cause",
            "owner",
            "playback-start"
        ]
    );
    assert_eq!(delivered.sources, 2);
    record(
        &mut fixture,
        "play-released",
        Expected {
            state: [1, 0, 1, 1, 0, 0, 0],
            scheduled: 0,
            terminal: None,
            terminals: 0,
            fault: false,
            offset_refs: 1,
            overflow_refs: 1,
            full: false,
        },
        Some(&overflow),
    );
    teardown(fixture);
    assert_eq!(Arc::strong_count(&overflow), 1);
}
