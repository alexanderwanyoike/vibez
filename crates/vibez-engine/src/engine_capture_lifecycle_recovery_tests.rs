//! Capture episode observations distinguish restored owners, away metadata and skipped validation.

use super::capture_lifecycle_admission_tests::{discard_setup, fixture, state, teardown, Fixture};
use super::*;
use graph_test_support::{routing_channel, routing_effect};
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;

fn set(fixture: &Fixture, index: usize, value: bool) -> EngineCommand {
    EngineCommand::SetEffectParam {
        track_id: fixture.tracks[0],
        effect_id: fixture.effect,
        param_index: index,
        value: f32::from(u8::from(value)),
    }
}

fn plan(fixture: &Fixture, extra: Option<EffectId>) -> Box<crate::routing::PreparedRouting> {
    let mut channel = routing_channel(fixture.tracks[0]);
    channel.effects.push(routing_effect(fixture.effect));
    if let Some(extra) = extra {
        channel.effects.push(routing_effect(extra));
    }
    crate::routing::PreparedRouting::prepare(
        &[
            channel,
            routing_channel(fixture.tracks[1]),
            routing_channel(TrackId::MASTER),
        ],
        64,
    )
    .unwrap()
}

fn identity(fixture: &Fixture) -> usize {
    reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect)
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

fn initial_closed(fixture: &mut Fixture) {
    fixture.commands.push(set(fixture, 0, true)).unwrap();
    zero(fixture);
    discard_setup(fixture);
    assert_eq!(state(&fixture.engine), [0, 1, 1, 1, 0, 0, 0]);
    fixture.calls.clear();
}

struct Expected {
    state: [u64; 7],
    flags: [u64; 7],
    order: Vec<&'static str>,
    fillers: usize,
    retired: Option<usize>,
}

fn record(fixture: &mut Fixture, case: &str, original_owner: usize, expected: Expected) {
    let observed_state = state(&fixture.engine);
    assert_eq!(observed_state, expected.state, "{case}");
    let pending = fixture.engine.device_handoff.pending();
    let mut order = Vec::new();
    let mut times = Vec::new();
    let mut fillers = 0;
    let mut retired = None;
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PlaybackPosition(u64::MAX) => fillers += 1,
            EngineEvent::DeviceReconfiguration(owner) => {
                assert!(fixture.owner.is_none());
                assert_eq!(owner.owner_identity(), original_owner);
                assert_eq!(owner.handoff_id(), 1);
                assert_eq!(owner.track_id(), fixture.tracks[0]);
                assert_eq!(owner.effect_id(), Some(fixture.effect));
                fixture.owner = Some(owner);
                order.push("owner");
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                order.push("stop");
                times.push(effective_at_samples);
            }
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
                order.push("start");
                times.push(effective_at_samples);
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                assert!(fixture.tracks.contains(&track_id));
                assert_eq!(position, 0);
                order.push("source");
                times.push(effective_at_samples);
            }
            EngineEvent::RoutingRetired(routing) => {
                assert!(retired.is_none());
                retired = Some(std::ptr::from_ref(routing.as_ref()) as usize);
                order.push("routing-retired");
            }
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture recovery event: {event:?}"),
        }
    }
    let flags = [
        u64::from(fixture.engine.device_handoff.is_suspended()),
        u64::from(pending.is_some()),
        u64::from(pending.is_some_and(|pending| pending.is_local_recovery())),
        u64::from(fixture.engine.compensation_plan.awaiting_structural_plan()),
        u64::from(fixture.engine.capture_stop_pending()),
        u64::from(fixture.owner.is_some()),
        fixture.engine.device_handoff.retained_parameters().len() as u64,
    ];
    let held_or_restored = fixture
        .owner
        .as_ref()
        .map(|owner| owner.owner_identity())
        .unwrap_or_else(|| identity(fixture));
    assert_eq!(held_or_restored, original_owner);
    assert_eq!(flags, expected.flags, "{case}");
    assert_eq!(order, expected.order, "{case}");
    assert_eq!(fillers, expected.fillers);
    assert_eq!(retired, expected.retired);
    assert!(times.iter().all(|time| *time == 0));
    let _calls = fixture.calls.observed();
    fixture.calls.clear();
}

#[test]
fn full_handoff_rollback_keeps_the_closed_episode_and_original_pending_stop() {
    let mut fixture = fixture();
    let original_owner = identity(&fixture);
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
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
    fixture.commands.push(set(&fixture, 0, true)).unwrap();
    fixture.commands.push(set(&fixture, 1, true)).unwrap();
    fixture.calls.clear();
    zero(&mut fixture);
    assert_eq!(fixture.engine.device_handoff.last_issued_identity(), 1);
    record(
        &mut fixture,
        "full-local-rollback",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [0, 0, 0, 0, 1, 0, 0],
            order: vec![],
            fillers,
            retired: None,
        },
    );
    fixture.commands.push(set(&fixture, 1, false)).unwrap();
    zero(&mut fixture);
    record(
        &mut fixture,
        "full-stop-released",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [0; 7],
            order: vec!["stop"],
            fillers: 0,
            retired: None,
        },
    );
    teardown(fixture);
}

#[test]
fn local_away_metadata_keeps_the_episode_closed_until_current_resume_is_validated() {
    let mut fixture = fixture();
    let original_owner = identity(&fixture);
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    discard_setup(&mut fixture);
    fixture.commands.push(set(&fixture, 0, true)).unwrap();
    fixture.commands.push(set(&fixture, 1, true)).unwrap();
    fixture.calls.clear();
    zero(&mut fixture);
    record(
        &mut fixture,
        "local-away",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [0, 1, 1, 0, 0, 1, 0],
            order: vec!["owner", "stop"],
            fillers: 0,
            retired: None,
        },
    );
    fixture.commands.push(set(&fixture, 0, false)).unwrap();
    fixture.commands.push(set(&fixture, 1, false)).unwrap();
    zero(&mut fixture);
    record(
        &mut fixture,
        "local-away-clear-retained",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [0, 1, 1, 0, 0, 1, 2],
            order: vec![],
            fillers: 0,
            retired: None,
        },
    );
    let retired = std::ptr::from_ref(fixture.engine.routing.as_ref()) as usize;
    let device = fixture.owner.take().unwrap();
    let routing = plan(&fixture, None);
    fixture
        .commands
        .push(EngineCommand::ResumeDeviceReconfiguration { device, routing })
        .unwrap();
    drain(&mut fixture);
    record(
        &mut fixture,
        "local-resume-before-validation",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [0; 7],
            order: vec!["routing-retired"],
            fillers: 0,
            retired: Some(retired),
        },
    );
    zero(&mut fixture);
    record(
        &mut fixture,
        "local-resume-clear-validation",
        original_owner,
        Expected {
            state: [0, 0, 1, 1, 0, 0, 0],
            flags: [0; 7],
            order: vec![],
            fillers: 0,
            retired: None,
        },
    );
    teardown(fixture);
}

#[test]
fn project_suspension_skips_clear_validation_without_changing_capture_admission() {
    let mut fixture = fixture();
    let original_owner = identity(&fixture);
    initial_closed(&mut fixture);
    fixture.commands.push(set(&fixture, 0, false)).unwrap();
    fixture.commands.push(set(&fixture, 1, true)).unwrap();
    zero(&mut fixture);
    record(
        &mut fixture,
        "suspended-clear-skipped",
        original_owner,
        Expected {
            state: [0, 1, 1, 1, 0, 0, 0],
            flags: [1, 1, 0, 0, 0, 1, 0],
            order: vec!["owner"],
            fillers: 0,
            retired: None,
        },
    );
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    zero(&mut fixture);
    record(
        &mut fixture,
        "suspended-start-clear-skipped",
        original_owner,
        Expected {
            state: [1, 1, 1, 1, 0, 0, 0],
            flags: [1, 1, 0, 0, 0, 1, 0],
            order: vec!["start", "source", "source"],
            fillers: 0,
            retired: None,
        },
    );
    fixture.commands.push(set(&fixture, 1, false)).unwrap();
    drain(&mut fixture);
    let retired = std::ptr::from_ref(fixture.engine.routing.as_ref()) as usize;
    let device = fixture.owner.take().unwrap();
    let routing = plan(&fixture, None);
    fixture
        .commands
        .push(EngineCommand::ResumeDeviceReconfiguration { device, routing })
        .unwrap();
    drain(&mut fixture);
    record(
        &mut fixture,
        "suspended-resume-before-validation",
        original_owner,
        Expected {
            state: [1, 1, 1, 1, 0, 0, 0],
            flags: [0; 7],
            order: vec!["routing-retired"],
            fillers: 0,
            retired: Some(retired),
        },
    );
    zero(&mut fixture);
    record(
        &mut fixture,
        "suspended-resume-clear-validation",
        original_owner,
        Expected {
            state: [1, 0, 1, 1, 0, 0, 0],
            flags: [0; 7],
            order: vec![],
            fillers: 0,
            retired: None,
        },
    );
    teardown(fixture);
}

#[test]
fn awaiting_and_invalid_plans_skip_clear_episode_observation_until_full_publication() {
    for invalid in [false, true] {
        let mut fixture = fixture();
        let original_owner = identity(&fixture);
        initial_closed(&mut fixture);
        let extra = EffectId::new();
        fixture.commands.push(set(&fixture, 0, false)).unwrap();
        fixture
            .commands
            .push(EngineCommand::insert_built_in_effect(
                fixture.tracks[0],
                extra,
                EffectType::Gain,
                DEFAULT_SAMPLE_RATE,
            ))
            .unwrap();
        if invalid {
            fixture
                .commands
                .push(EngineCommand::RejectRoutingUpdate {
                    reason: "Original Capture episode preguard".into(),
                })
                .unwrap();
        }
        fixture.engine.drain_commands();
        discard_setup(&mut fixture);
        fixture.calls.clear();
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.validate_compensation()),
            (0, 0)
        );
        assert!(fixture.calls.observed().is_empty());
        record(
            &mut fixture,
            if invalid {
                "invalid-clear-skipped"
            } else {
                "awaiting-clear-skipped"
            },
            original_owner,
            Expected {
                state: [0, 1, u64::from(!invalid), u64::from(!invalid), 0, 0, 0],
                flags: [0, 0, 0, 1, 0, 0, 0],
                order: vec![],
                fillers: 0,
                retired: None,
            },
        );
        let retired = std::ptr::from_ref(fixture.engine.routing.as_ref()) as usize;
        let routing = plan(&fixture, Some(extra));
        fixture
            .commands
            .push(EngineCommand::SetRouting(routing))
            .unwrap();
        drain(&mut fixture);
        assert!(fixture
            .engine
            .capture_lifecycle
            .local_failure_closure_requested());
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.validate_compensation()),
            (0, 0)
        );
        record(
            &mut fixture,
            if invalid {
                "invalid-full-plan-clear"
            } else {
                "awaiting-full-plan-clear"
            },
            original_owner,
            Expected {
                state: [0, 0, 1, u64::from(!invalid), 0, 0, 0],
                flags: [0; 7],
                order: vec!["routing-retired"],
                fillers: 0,
                retired: Some(retired),
            },
        );
        teardown(fixture);
    }
}
