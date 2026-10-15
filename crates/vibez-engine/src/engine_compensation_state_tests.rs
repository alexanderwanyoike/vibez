//! Command admission preserves independent plan, device and presentation readiness.

use super::*;
use compensation_state_test_support::{Fixture, Readiness, StartingCondition};
use std::sync::atomic::Ordering;
use vibez_core::midi::InstrumentKind;

const RUNNING: Readiness = Readiness {
    valid: true,
    awaiting_plan: false,
    suspended: false,
    handoff: false,
    restart_requested: false,
    presentation_fault: false,
    playing: true,
    muted: false,
};
const WAITING: Readiness = Readiness {
    awaiting_plan: true,
    muted: true,
    ..RUNNING
};
const REQUESTED: Readiness = Readiness {
    restart_requested: true,
    muted: true,
    ..RUNNING
};
const AWAY: Readiness = Readiness {
    suspended: true,
    handoff: true,
    muted: true,
    ..RUNNING
};
const INVALID: Readiness = Readiness {
    valid: false,
    playing: false,
    muted: true,
    ..RUNNING
};
const FAULTED: Readiness = Readiness {
    presentation_fault: true,
    playing: false,
    muted: true,
    ..RUNNING
};

#[derive(Debug, Clone, Copy)]
enum Command {
    Play,
    Stop,
    Seek,
    Routing,
    AutomationRouting,
    RejectRouting,
    SetInstrument,
    RemoveInstrument,
}

impl Command {
    fn prepare(self, fixture: &Fixture) -> EngineCommand {
        match self {
            Self::Play => EngineCommand::Play,
            Self::Stop => EngineCommand::Stop,
            Self::Seek => EngineCommand::Seek(123),
            Self::Routing => {
                EngineCommand::SetRouting(fixture.plan(fixture.reported.load(Ordering::Acquire)))
            }
            Self::AutomationRouting => EngineCommand::UpdateAutomationRouting(
                fixture.plan(fixture.reported.load(Ordering::Acquire)),
            ),
            Self::RejectRouting => EngineCommand::RejectRoutingUpdate {
                reason: "characterized routing rejection".into(),
            },
            Self::SetInstrument => EngineCommand::set_built_in_instrument(
                fixture.tracks[1],
                InstrumentKind::SubtractiveSynth,
                DEFAULT_SAMPLE_RATE,
            ),
            Self::RemoveInstrument => EngineCommand::RemoveTrackInstrument(fixture.tracks[1]),
        }
    }
}

#[test]
fn reachable_readiness_by_command_transition_table() {
    let commands = [
        Command::Play,
        Command::Stop,
        Command::Seek,
        Command::Routing,
        Command::AutomationRouting,
        Command::RejectRouting,
        Command::SetInstrument,
        Command::RemoveInstrument,
    ];
    let rows = [
        (
            StartingCondition::Running,
            RUNNING,
            [
                RUNNING,
                Readiness {
                    playing: false,
                    ..RUNNING
                },
                RUNNING,
                RUNNING,
                RUNNING,
                RUNNING,
                RUNNING,
                RUNNING,
            ],
        ),
        (
            StartingCondition::AwaitingPlan,
            WAITING,
            [
                WAITING,
                Readiness {
                    playing: false,
                    ..WAITING
                },
                WAITING,
                RUNNING,
                WAITING,
                Readiness {
                    awaiting_plan: true,
                    ..INVALID
                },
                WAITING,
                WAITING,
            ],
        ),
        (
            StartingCondition::RestartRequested,
            REQUESTED,
            [
                REQUESTED,
                Readiness {
                    playing: false,
                    ..REQUESTED
                },
                REQUESTED,
                REQUESTED,
                REQUESTED,
                REQUESTED,
                REQUESTED,
                REQUESTED,
            ],
        ),
        (
            StartingCondition::DeviceAway,
            AWAY,
            [
                AWAY,
                Readiness {
                    playing: false,
                    ..AWAY
                },
                AWAY,
                AWAY,
                AWAY,
                AWAY,
                AWAY,
                AWAY,
            ],
        ),
        (
            StartingCondition::InvalidPlan,
            INVALID,
            [
                Readiness {
                    playing: true,
                    ..INVALID
                },
                INVALID,
                INVALID,
                Readiness {
                    playing: false,
                    ..RUNNING
                },
                INVALID,
                INVALID,
                INVALID,
                INVALID,
            ],
        ),
        (
            StartingCondition::PresentationFault,
            FAULTED,
            [
                RUNNING, FAULTED, FAULTED, FAULTED, FAULTED, FAULTED, FAULTED, FAULTED,
            ],
        ),
    ];
    for (start, initial, expected) in rows {
        for (command, expected) in commands.into_iter().zip(expected) {
            let mut fixture = Fixture::new(start);
            assert_eq!(
                Readiness::observe(&fixture.engine),
                initial,
                "initial {start:?}"
            );
            let prepared = command.prepare(&fixture);
            fixture.drain_command(prepared);
            assert_eq!(
                Readiness::observe(&fixture.engine),
                expected,
                "{start:?} + {command:?}"
            );
            if matches!(command, Command::Seek) {
                assert_eq!(fixture.engine.transport.position(), 123);
            }
            let rejected = fixture.drain_events().into_iter().any(|event| matches!(event,
                EngineEvent::CompensationFailed { reason } if reason == "characterized routing rejection"));
            assert_eq!(
                rejected,
                matches!(command, Command::RejectRouting),
                "{start:?} + {command:?}"
            );
        }
    }
}

#[test]
fn unchanged_instrument_lifetimes_do_not_latch_a_structural_wait() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    let graph = std::ptr::from_ref(fixture.engine.routing.as_ref());
    for kind in [
        Some(InstrumentKind::SubtractiveSynth),
        Some(InstrumentKind::Sampler),
        None,
    ] {
        let command = kind.map_or(
            EngineCommand::RemoveTrackInstrument(fixture.tracks[1]),
            |kind| {
                EngineCommand::set_built_in_instrument(fixture.tracks[1], kind, DEFAULT_SAMPLE_RATE)
            },
        );
        fixture.drain_command(command);
        assert_eq!(Readiness::observe(&fixture.engine), RUNNING);
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), graph);
        assert_eq!(
            fixture.engine.tracks[1]
                .instrument
                .as_ref()
                .map(|instrument| instrument.instrument_kind()),
            kind
        );
        fixture.drain_events();
    }
}

#[test]
fn current_device_reply_restores_the_same_owner_or_fails_the_project() {
    for rejected in [false, true] {
        let mut fixture = Fixture::new(StartingCondition::DeviceAway);
        let mut owner = fixture.owner.take().unwrap();
        let identity = owner.owner_identity();
        let command = if rejected {
            EngineCommand::RejectDeviceReconfiguration {
                device: owner,
                reason: "characterized device rejection".into(),
            }
        } else {
            owner.reconfigure_on_main_thread().unwrap();
            EngineCommand::ResumeDeviceReconfiguration {
                routing: fixture.plan(owner.latency_samples()),
                device: owner,
            }
        };
        fixture.drain_command(command);
        assert!(!fixture.engine.compensation_suspended);
        assert!(fixture.engine.pending_device_reconfiguration.is_none());
        assert_eq!(
            reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect),
            identity
        );
        if rejected {
            assert!(!fixture.engine.compensation_plan.applied_plan_is_valid());
            assert!(!fixture.engine.transport.is_playing());
            assert!(fixture.engine.project_processing_muted());
        } else {
            assert_eq!(Readiness::observe(&fixture.engine), RUNNING);
        }
        let events = fixture.drain_events();
        assert_eq!(events.iter().filter(|event| matches!(event,
            EngineEvent::CompensationFailed { reason } if reason == "characterized device rejection")).count(), usize::from(rejected));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, EngineEvent::PlaybackStopped))
                .count(),
            usize::from(rejected)
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, EngineEvent::DeviceReconfigurationRetired { .. })));
    }
}

#[test]
fn invalidating_an_away_device_makes_both_late_reply_kinds_retire_without_revival() {
    for rejected in [false, true] {
        let mut fixture = Fixture::new(StartingCondition::DeviceAway);
        let owner = fixture.owner.take().unwrap();
        let identity = owner.owner_identity();
        fixture.drain_command(EngineCommand::RemoveEffect(
            fixture.tracks[0],
            fixture.effect,
        ));
        assert_eq!(Readiness::observe(&fixture.engine), WAITING);
        fixture.drain_events();
        let command = if rejected {
            EngineCommand::RejectDeviceReconfiguration {
                device: owner,
                reason: "stale restart".into(),
            }
        } else {
            EngineCommand::ResumeDeviceReconfiguration {
                device: owner,
                routing: fixture.plan(0),
            }
        };
        fixture.drain_command(command);
        assert_eq!(Readiness::observe(&fixture.engine), WAITING);
        assert!(fixture.engine.tracks[0].effects.is_empty());
        let events = fixture.drain_events();
        let retired: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::DeviceReconfigurationRetired { device, reason } => {
                    Some((device.owner_identity(), reason.as_deref()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(retired, [(identity, rejected.then_some("stale restart"))]);
        assert!(!events.iter().any(|event| matches!(
            event,
            EngineEvent::CompensationInvalid { .. }
                | EngineEvent::CompensationFailed { .. }
                | EngineEvent::PlaybackStarted
                | EngineEvent::PlaybackStopped
        )));
    }
}

#[test]
fn an_unrelated_structural_edit_and_plan_publication_do_not_complete_an_away_device() {
    let mut fixture = Fixture::new(StartingCondition::DeviceAway);
    let effect = vibez_core::id::EffectId::new();
    let command = EngineCommand::insert_built_in_effect(
        fixture.tracks[1],
        effect,
        vibez_core::effect::EffectType::Gain,
        DEFAULT_SAMPLE_RATE,
    );
    fixture.drain_command(command);
    assert_eq!(
        Readiness::observe(&fixture.engine),
        Readiness {
            awaiting_plan: true,
            ..AWAY
        }
    );
    let plan = fixture.plan(0);
    fixture.drain_command(EngineCommand::SetRouting(plan));
    assert_eq!(Readiness::observe(&fixture.engine), AWAY);
    let mut owner = fixture.owner.take().unwrap();
    owner.reconfigure_on_main_thread().unwrap();
    let plan = fixture.plan(owner.latency_samples());
    fixture.drain_command(EngineCommand::ResumeDeviceReconfiguration {
        device: owner,
        routing: plan,
    });
    assert_eq!(Readiness::observe(&fixture.engine), RUNNING);
    assert!(fixture
        .engine
        .routing
        .graph
        .index(
            fixture.tracks[1],
            vibez_core::routing::NodeStage::Effect(effect)
        )
        .is_some());
}

#[test]
fn a_full_ui_ring_retains_a_restart_request_and_retries_the_same_owner() {
    let mut fixture = Fixture::new(StartingCondition::RestartRequested);
    let identity =
        reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect);
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.begin_device_reconfiguration()),
        (0, 0)
    );
    assert_eq!(Readiness::observe(&fixture.engine), REQUESTED);
    assert_eq!(
        reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect),
        identity
    );
    fixture.drain_events();
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.begin_device_reconfiguration()),
        (0, 0)
    );
    assert_eq!(Readiness::observe(&fixture.engine), AWAY);
    let owners: Vec<_> = fixture
        .drain_events()
        .into_iter()
        .filter_map(|event| match event {
            EngineEvent::DeviceReconfiguration(device) => Some(device),
            _ => None,
        })
        .collect();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].owner_identity(), identity);
    assert_eq!(owners[0].handoff_id(), 2);
}

#[test]
fn full_ui_routing_admission_retains_rejected_plan_until_retired_graph_returns() {
    let mut fixture = Fixture::new(StartingCondition::Running);
    fixture.engine.process(&mut [], 2);
    fixture.drain_events();
    let capture_stop = fixture.engine.compensation_callback_heard_start;
    assert_eq!(capture_stop, 64);
    let original = std::ptr::from_ref(fixture.engine.routing.as_ref());
    let effect_identity =
        reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect);
    fixture.drain_command(EngineCommand::RemoveEffect(
        fixture.tracks[0],
        fixture.effect,
    ));
    assert_eq!(Readiness::observe(&fixture.engine), WAITING);
    let mut removed = 0;
    for event in fixture.drain_events() {
        match event {
            EngineEvent::DisposeEffect(owner) => {
                let effect = owner.take();
                assert_eq!(
                    reconfiguration::effect_owner_identity(&*effect),
                    effect_identity
                );
                removed += 1;
            }
            event => panic!("unexpected effect retirement: {event:?}"),
        }
    }
    assert_eq!(removed, 1);
    while fixture
        .engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    let reason = String::from("full UI retains the rejected structural plan reason");
    let reason_identity = reason.as_ptr();
    fixture.drain_command(EngineCommand::RejectRoutingUpdate { reason });
    let invalid_waiting = Readiness {
        awaiting_plan: true,
        ..INVALID
    };
    assert_eq!(Readiness::observe(&fixture.engine), invalid_waiting);
    assert_eq!(
        std::ptr::from_ref(fixture.engine.routing.as_ref()),
        original
    );
    let automation = fixture.plan(0);
    let automation_identity = std::ptr::from_ref(automation.as_ref());
    fixture.drain_command(EngineCommand::UpdateAutomationRouting(automation));
    assert_eq!(Readiness::observe(&fixture.engine), invalid_waiting);
    assert_eq!(
        std::ptr::from_ref(fixture.engine.routing.as_ref()),
        automation_identity
    );
    assert_eq!(
        fixture
            .engine
            .retired_routing
            .as_ref()
            .map(|plan| std::ptr::from_ref(plan.as_ref())),
        Some(original)
    );
    assert_eq!(fixture.engine.pending_retirements.len(), 1);
    assert!(matches!(
        &fixture.engine.pending_retirements[0],
        EngineEvent::CompensationFailed { reason } if reason.as_ptr() == reason_identity
    ));
    let accepted = fixture.plan(0);
    let accepted_identity = std::ptr::from_ref(accepted.as_ref());
    fixture
        .commands
        .push(EngineCommand::SetRouting(accepted))
        .unwrap();
    for released in 0..=3 {
        if released != 0 {
            assert!(matches!(
                fixture.events.pop().unwrap(),
                EngineEvent::PlaybackPosition(0)
            ));
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
            (0, 0)
        );
        let observed = Readiness::observe(&fixture.engine);
        assert_eq!(fixture.engine.event_tx.slots(), 0);
        assert_eq!(fixture.engine.pending_retirements.len(), 1);
        assert!(matches!(
            &fixture.engine.pending_retirements[0],
            EngineEvent::CompensationFailed { reason } if reason.as_ptr() == reason_identity
        ));
        if released < 3 {
            assert_eq!(observed, invalid_waiting);
            assert!(matches!(
                fixture.engine.cmd_rx.peek(),
                Ok(EngineCommand::SetRouting(plan)) if std::ptr::from_ref(plan.as_ref()) == accepted_identity
            ));
            assert_eq!(
                std::ptr::from_ref(fixture.engine.routing.as_ref()),
                automation_identity
            );
            assert_eq!(
                fixture
                    .engine
                    .retired_routing
                    .as_ref()
                    .map(|plan| std::ptr::from_ref(plan.as_ref())),
                Some(original)
            );
        } else {
            assert_eq!(
                observed,
                Readiness {
                    playing: false,
                    ..RUNNING
                }
            );
            assert!(fixture.engine.cmd_rx.peek().is_err());
            assert_eq!(
                std::ptr::from_ref(fixture.engine.routing.as_ref()),
                accepted_identity
            );
            assert_eq!(
                fixture
                    .engine
                    .retired_routing
                    .as_ref()
                    .map(|plan| std::ptr::from_ref(plan.as_ref())),
                Some(automation_identity)
            );
        }
    }
    let mut order = Vec::new();
    for event in std::iter::from_fn(|| fixture.events.pop().ok()) {
        match event {
            EngineEvent::PlaybackPosition(0) => {}
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                assert_eq!(effective_at_samples, capture_stop);
                order.push("capture-stop");
            }
            EngineEvent::PlaybackStopped => order.push("playback-stop"),
            EngineEvent::RoutingRetired(plan) => {
                assert_eq!(std::ptr::from_ref(plan.as_ref()), original);
                order.push("original-plan-return");
            }
            event => panic!("unexpected first released event: {event:?}"),
        }
    }
    assert_eq!(
        order,
        ["capture-stop", "playback-stop", "original-plan-return"]
    );
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    for event in std::iter::from_fn(|| fixture.events.pop().ok()) {
        match event {
            EngineEvent::RoutingRetired(plan) => {
                assert_eq!(std::ptr::from_ref(plan.as_ref()), automation_identity);
                order.push("automation-plan-return");
            }
            EngineEvent::CompensationFailed { reason } => {
                assert_eq!(
                    reason,
                    "full UI retains the rejected structural plan reason"
                );
                assert_eq!(reason.as_ptr(), reason_identity);
                order.push("rejection");
            }
            event => panic!("unexpected remaining released event: {event:?}"),
        }
    }
    assert_eq!(
        order,
        [
            "capture-stop",
            "playback-stop",
            "original-plan-return",
            "automation-plan-return",
            "rejection"
        ]
    );
    assert!(fixture.engine.retired_routing.is_none());
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(
        std::ptr::from_ref(fixture.engine.routing.as_ref()),
        accepted_identity
    );
    assert_eq!(
        Readiness::observe(&fixture.engine),
        Readiness {
            playing: false,
            ..RUNNING
        }
    );
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    assert!(fixture.events.pop().is_err());
    assert!(fixture.drain_events().is_empty());
}

#[test]
fn capture_admission_uses_applied_plan_validity_instead_of_global_processing_gate() {
    for start in [
        StartingCondition::Running,
        StartingCondition::AwaitingPlan,
        StartingCondition::DeviceAway,
        StartingCondition::PresentationFault,
        StartingCondition::InvalidPlan,
    ] {
        let mut fixture = Fixture::new(start);
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut [], 2)),
            (0, 0)
        );
        fixture.drain_events();
        let before = Readiness::observe(&fixture.engine);
        let allowed = !matches!(start, StartingCondition::InvalidPlan);
        assert_eq!(before.valid, allowed);
        assert_eq!(before.muted, !matches!(start, StartingCondition::Running));
        assert!(!fixture.engine.local_device_failure_pending());
        assert!(!fixture.engine.capture_active);
        let capture_start = fixture.engine.heard_capture_position();
        assert_eq!(capture_start, 64);
        let plan = std::ptr::from_ref(fixture.engine.routing.as_ref());
        fixture.drain_command(EngineCommand::StartPerformanceCapture);
        let after = Readiness::observe(&fixture.engine);
        assert_eq!(after, before, "{start:?}");
        assert_eq!(fixture.engine.capture_active, allowed, "{start:?}");
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), plan);
        let mut order = Vec::new();
        let mut source_count = 0;
        for event in fixture.drain_events() {
            match event {
                EngineEvent::PerformanceCaptureStarted {
                    offsets,
                    effective_at_samples,
                    section_id,
                    section_position_samples,
                } => {
                    assert!(allowed, "{start:?}");
                    assert_eq!(effective_at_samples, capture_start);
                    assert_eq!(section_id, None);
                    assert_eq!(section_position_samples, None);
                    assert!(Arc::ptr_eq(
                        &offsets,
                        &fixture.engine.routing.capture_offsets
                    ));
                    order.push("capture-start");
                }
                EngineEvent::ClipCaptureSource {
                    track_id,
                    position,
                    effective_at_samples,
                } => {
                    assert!(allowed, "{start:?}");
                    assert_eq!(track_id, fixture.tracks[source_count]);
                    assert_eq!(position, 0);
                    assert_eq!(effective_at_samples, capture_start);
                    source_count += 1;
                    order.push("capture-source");
                }
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                } => {
                    assert!(!allowed, "{start:?}");
                    assert_eq!(effective_at_samples, capture_start);
                    order.push("capture-stop");
                }
                EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id,
                    reason,
                } => {
                    assert!(!allowed, "{start:?}");
                    assert_eq!(track_id, TrackId::MASTER);
                    assert_eq!(effect_id, None);
                    assert_eq!(
                        reason,
                        "Capture requires a valid applied audio configuration"
                    );
                    order.push("invalid-plan-cause");
                }
                event => panic!("unexpected {start:?} Capture event: {event:?}"),
            }
        }
        if allowed {
            assert_eq!(order, ["capture-start", "capture-source", "capture-source"]);
            assert_eq!(source_count, 2);
            fixture.drain_command(EngineCommand::StopPerformanceCapture);
            assert!(!fixture.engine.capture_active);
            assert_eq!(Readiness::observe(&fixture.engine), before);
            let stopped = fixture.drain_events();
            assert_eq!(stopped.len(), 1);
            assert!(matches!(
                stopped[0],
                EngineEvent::PerformanceCaptureStopped { effective_at_samples } if effective_at_samples == capture_start
            ));
            order.push("capture-stop");
        } else {
            assert_eq!(order, ["capture-stop", "invalid-plan-cause"]);
            assert_eq!(source_count, 0);
        }
        assert_eq!(std::ptr::from_ref(fixture.engine.routing.as_ref()), plan);
        assert!(!fixture.engine.capture_active);
        assert!(fixture.drain_events().is_empty());
    }
}
