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
