//! Local Capture closure rearms at accepted Start or clear validation while global actions stay independent.

use super::capture_lifecycle_admission_tests::{discard_setup, fixture, state, teardown, Fixture};
use super::*;

fn set(fixture: &Fixture, instrument: bool, index: usize, value: bool) -> EngineCommand {
    if instrument {
        EngineCommand::SetInstrumentParam {
            track_id: fixture.tracks[0],
            param_index: index,
            value: f32::from(u8::from(value)),
        }
    } else {
        EngineCommand::SetEffectParam {
            track_id: fixture.tracks[0],
            effect_id: fixture.effect,
            param_index: index,
            value: f32::from(u8::from(value)),
        }
    }
}

#[derive(Default)]
struct Events {
    order: Vec<&'static str>,
    times: Vec<u64>,
    sources: Vec<(usize, u64)>,
    invalid_targets: Vec<u32>,
    offset_owners: usize,
}

fn collect(fixture: &mut Fixture, boundary: u64) -> Events {
    let mut observed = Events::default();
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
                observed.order.push("start");
                observed.times.push(effective_at_samples);
                observed.offset_owners += 1;
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                let index = fixture
                    .tracks
                    .iter()
                    .position(|track| *track == track_id)
                    .unwrap();
                observed.order.push("source");
                observed.times.push(effective_at_samples);
                observed.sources.push((index, position));
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                observed.order.push("stop");
                observed.times.push(effective_at_samples);
            }
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!(track_id, fixture.tracks[0]);
                assert_eq!(
                    reason,
                    "Device processing configuration is unavailable or failed"
                );
                observed.invalid_targets.push(if let Some(id) = effect_id {
                    assert_eq!(id, fixture.effect);
                    2
                } else {
                    1
                });
                observed.order.push("invalid");
            }
            EngineEvent::PlaybackStopped => observed.order.push("playback-stop"),
            EngineEvent::TrackMeter(_)
            | EngineEvent::Metering(_)
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture episode event: {event:?}"),
        }
    }
    assert!(observed.times.iter().all(|time| *time == boundary));
    observed
}

struct Expected {
    before: [u64; 7],
    after: [u64; 7],
    order: Vec<&'static str>,
    invalid_targets: Vec<u32>,
    starts: usize,
}

fn phase(fixture: &mut Fixture, case: &str, expected: Expected) {
    let before = state(&fixture.engine);
    assert_eq!(before, expected.before, "{case}");
    let offsets = Arc::strong_count(&fixture.engine.routing.capture_offsets);
    fixture.calls.clear();
    let mut output = [0.0; 16];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut output, 2))),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
    let after = state(&fixture.engine);
    assert_eq!(after, expected.after, "{case}");
    let calls = fixture.calls.observed();
    assert!(!calls.is_empty());
    assert!(calls
        .iter()
        .all(|call| matches!(*call, 110 | 111 | 210 | 211)));
    assert_eq!(
        Arc::strong_count(&fixture.engine.routing.capture_offsets),
        offsets + expected.starts
    );
    let observed = collect(fixture, before[5]);
    assert_eq!(observed.order, expected.order, "{case}");
    assert_eq!(observed.invalid_targets, expected.invalid_targets, "{case}");
    assert_eq!(observed.offset_owners, expected.starts);
    assert_eq!(observed.sources, [(0, 0), (1, 0)].repeat(expected.starts));
    assert_eq!(
        Arc::strong_count(&fixture.engine.routing.capture_offsets),
        offsets
    );
    for frame in output.as_chunks::<2>().0 {
        assert_eq!(
            *frame,
            [if after[2] != 0 { 0.25 } else { 0.0 }, 0.0],
            "{case}"
        );
    }
    assert!(fixture.owner.is_none());
}

fn playing(active: bool, closed: bool, frames: u64) -> [u64; 7] {
    [
        u64::from(active),
        u64::from(closed),
        1,
        1,
        frames,
        frames,
        frames,
    ]
}

#[test]
fn persistent_and_two_device_failures_close_once_between_recovery_boundaries() {
    let mut fixture = fixture();
    let cases = [
        (
            "first-local",
            false,
            false,
            false,
            true,
            true,
            false,
            true,
            vec!["start", "source", "source", "stop"],
        ),
        (
            "persistent-local",
            false,
            true,
            false,
            true,
            false,
            false,
            true,
            vec![],
        ),
        (
            "clear-validation",
            false,
            true,
            false,
            false,
            false,
            false,
            false,
            vec![],
        ),
        (
            "both-local",
            false,
            false,
            true,
            true,
            true,
            false,
            true,
            vec!["start", "source", "source", "stop"],
        ),
        (
            "one-remains-local",
            false,
            true,
            false,
            true,
            false,
            false,
            true,
            vec![],
        ),
        (
            "both-clear",
            false,
            true,
            false,
            false,
            false,
            false,
            false,
            vec![],
        ),
        (
            "idle-refailure",
            false,
            false,
            true,
            false,
            false,
            false,
            true,
            vec!["stop"],
        ),
    ];
    for (
        index,
        (
            case,
            active_before,
            closed_before,
            instrument_local,
            effect_local,
            start,
            active_after,
            closed_after,
            order,
        ),
    ) in cases.into_iter().enumerate()
    {
        fixture
            .commands
            .push(set(&fixture, true, 0, instrument_local))
            .unwrap();
        fixture
            .commands
            .push(set(&fixture, false, 0, effect_local))
            .unwrap();
        if start {
            fixture
                .commands
                .push(set(&fixture, true, 0, false))
                .unwrap();
            fixture
                .commands
                .push(set(&fixture, false, 0, false))
                .unwrap();
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            fixture
                .commands
                .push(set(&fixture, true, 0, instrument_local))
                .unwrap();
            fixture
                .commands
                .push(set(&fixture, false, 0, effect_local))
                .unwrap();
        }
        phase(
            &mut fixture,
            case,
            Expected {
                before: playing(active_before, closed_before, index as u64 * 8),
                after: playing(active_after, closed_after, (index as u64 + 1) * 8),
                order,
                invalid_targets: vec![],
                starts: usize::from(start),
            },
        );
    }
    teardown(fixture);
}

#[test]
fn clear_start_refail_closes_the_new_capture_with_or_without_prior_clear_validation() {
    for clear_validation in [false, true] {
        let mut fixture = fixture();
        fixture
            .commands
            .push(set(&fixture, false, 0, true))
            .unwrap();
        fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2));
        discard_setup(&mut fixture);
        assert_eq!(state(&fixture.engine), playing(false, true, 0));
        if clear_validation {
            fixture
                .commands
                .push(set(&fixture, false, 0, false))
                .unwrap();
            fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut [], 2));
            discard_setup(&mut fixture);
            assert_eq!(state(&fixture.engine), playing(false, false, 0));
        }
        fixture
            .commands
            .push(set(&fixture, false, 0, false))
            .unwrap();
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        fixture
            .commands
            .push(set(&fixture, false, 0, true))
            .unwrap();
        phase(
            &mut fixture,
            if clear_validation {
                "clear-before-start-refail"
            } else {
                "same-drain-start-refail"
            },
            Expected {
                before: playing(false, !clear_validation, 0),
                after: playing(false, true, 8),
                order: vec!["start", "source", "source", "stop"],
                invalid_targets: vec![],
                starts: 1,
            },
        );
        teardown(fixture);
    }
}

#[test]
fn first_invalid_order_preserves_local_global_overlap_without_universal_stop_deduplication() {
    for local_first in [true, false] {
        let mut fixture = fixture();
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        fixture
            .commands
            .push(set(&fixture, local_first, 0, true))
            .unwrap();
        fixture
            .commands
            .push(set(&fixture, !local_first, 2, false))
            .unwrap();
        let mut order = vec!["start", "source", "source"];
        if local_first {
            order.push("stop");
        }
        order.extend(["stop", "invalid", "playback-stop"]);
        phase(
            &mut fixture,
            if local_first {
                "local-before-invalid"
            } else {
                "invalid-before-local"
            },
            Expected {
                before: playing(false, false, 0),
                after: [0, u64::from(local_first), 0, 0, 0, 8, 8],
                order,
                invalid_targets: vec![if local_first { 2 } else { 1 }],
                starts: 1,
            },
        );
        teardown(fixture);
    }
}
