//! Rearmed local Capture closure retains accepted feed and the first pending stop under real UI pressure.

use super::capture_lifecycle_admission_tests::{discard_setup, fixture, state, teardown, Fixture};
use super::capture_lifecycle_transport_tests::{drain, local};
use super::*;

fn zero(fixture: &mut Fixture) {
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
}

#[derive(Default)]
struct Delivered {
    order: Vec<&'static str>,
    times: Vec<u64>,
    sources: usize,
    fillers: usize,
    offset_owners: usize,
}

fn collect(fixture: &mut Fixture, delivered: &mut Delivered) {
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
                delivered.offset_owners += 1;
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
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture rearm pressure event: {event:?}"),
        }
    }
    assert!(delivered.times.iter().all(|time| *time == 8));
}

struct Expected {
    active: bool,
    closed: bool,
    scheduled: usize,
    terminal: Option<u64>,
    full: bool,
    offset_refs: usize,
}

fn record(fixture: &mut Fixture, phase: &str, expected: Expected, expected_calls: &[u32]) {
    let observed = state(&fixture.engine);
    assert_eq!(
        observed,
        [
            u64::from(expected.active),
            u64::from(expected.closed),
            1,
            1,
            8,
            8,
            8
        ],
        "{phase}"
    );
    let scheduled = fixture.engine.scheduled_presentation.len();
    let terminal = fixture.engine.presentation_state.terminals.capture_stop();
    let slots = fixture.engine.event_tx.slots();
    let offset_refs = Arc::strong_count(&fixture.engine.routing.capture_offsets);
    let calls = fixture.calls.observed();
    assert_eq!(scheduled, expected.scheduled, "{phase}");
    assert_eq!(terminal, expected.terminal, "{phase}");
    assert_eq!(
        fixture.engine.presentation_state.terminals.occupied_slots(),
        usize::from(expected.terminal.is_some())
    );
    assert_eq!(
        fixture.engine.capture_stop_pending(),
        expected.terminal.is_some()
    );
    assert_eq!(
        slots,
        if expected.full {
            0
        } else {
            RING_BUFFER_CAPACITY
        }
    );
    assert_eq!(offset_refs, expected.offset_refs, "{phase}");
    assert_eq!(calls, expected_calls, "{phase}");
    assert!(!fixture.engine.presentation_state.is_faulted());
    assert!(fixture.owner.is_none());
    fixture.calls.clear();
}

#[test]
fn same_drain_refailure_closes_capture_while_an_old_stop_waits_for_real_ui_credits() {
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
    fixture.commands.push(local(&fixture, true)).unwrap();
    zero(&mut fixture);
    assert_eq!(state(&fixture.engine), [0, 1, 1, 1, 8, 8, 8]);
    assert_eq!(
        fixture.engine.presentation_state.terminals.capture_stop(),
        Some(8)
    );
    fixture.commands.push(local(&fixture, false)).unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    fixture.commands.push(local(&fixture, true)).unwrap();
    fixture.calls.clear();
    drain(&mut fixture);
    let mut delivered = Delivered::default();
    record(
        &mut fixture,
        "admitted",
        Expected {
            active: true,
            closed: false,
            scheduled: 3,
            terminal: Some(8),
            full: true,
            offset_refs: 2,
        },
        &[110, 210],
    );

    // Credits return to the real ring; retained feed still precedes the original terminal slot.
    for (phase, credits, published) in [
        ("zero-credit", 0, 0),
        ("one-credit", 1, 1),
        ("two-credits", 2, 3),
    ] {
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
            phase,
            Expected {
                active: false,
                closed: true,
                scheduled: 3 - published,
                terminal: Some(8),
                full: true,
                offset_refs: 2,
            },
            &[110, 211, 110, 211],
        );
    }
    collect(&mut fixture, &mut delivered);
    assert_eq!(delivered.fillers, RING_BUFFER_CAPACITY - 3);
    assert_eq!(delivered.order, ["start", "source", "source"]);
    assert_eq!(delivered.sources, 2);
    assert_eq!(delivered.offset_owners, 1);
    record(
        &mut fixture,
        "feed-drained",
        Expected {
            active: false,
            closed: true,
            scheduled: 0,
            terminal: Some(8),
            full: false,
            offset_refs: 1,
        },
        &[],
    );

    zero(&mut fixture);
    collect(&mut fixture, &mut delivered);
    assert_eq!(delivered.order, ["start", "source", "source", "stop"]);
    record(
        &mut fixture,
        "stop-published",
        Expected {
            active: false,
            closed: true,
            scheduled: 0,
            terminal: None,
            full: false,
            offset_refs: 1,
        },
        &[110, 211, 110, 211],
    );
    zero(&mut fixture);
    collect(&mut fixture, &mut delivered);
    assert_eq!(delivered.order, ["start", "source", "source", "stop"]);
    record(
        &mut fixture,
        "persistent-after-stop",
        Expected {
            active: false,
            closed: true,
            scheduled: 0,
            terminal: None,
            full: false,
            offset_refs: 1,
        },
        &[110, 211, 110, 211],
    );

    fixture
        .commands
        .push(EngineCommand::StopPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    collect(&mut fixture, &mut delivered);
    assert_eq!(
        delivered.order,
        ["start", "source", "source", "stop", "stop"]
    );
    record(
        &mut fixture,
        "public-stop",
        Expected {
            active: false,
            closed: true,
            scheduled: 0,
            terminal: None,
            full: false,
            offset_refs: 1,
        },
        &[],
    );
    zero(&mut fixture);
    collect(&mut fixture, &mut delivered);
    assert_eq!(
        delivered.order,
        ["start", "source", "source", "stop", "stop"]
    );
    assert_eq!(delivered.sources, 2);
    assert_eq!(delivered.offset_owners, 1);
    record(
        &mut fixture,
        "persistent-after-public-stop",
        Expected {
            active: false,
            closed: true,
            scheduled: 0,
            terminal: None,
            full: false,
            offset_refs: 1,
        },
        &[110, 211, 110, 211],
    );
    teardown(fixture);
}
