//! Reachable engine fixtures for compensation and presentation transition checks.

use super::*;
use crate::test_support::DelayProbe;
use graph_test_support::{constant_clip, graph_engine, routing_channel, routing_effect};
use std::sync::atomic::{AtomicU32, Ordering};
use vibez_core::{
    id::EffectId,
    routing::{NodeStage, RoutingNode},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartingCondition {
    Running,
    AwaitingPlan,
    RestartRequested,
    DeviceAway,
    InvalidPlan,
    PresentationFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Readiness {
    pub valid: bool,
    pub awaiting_plan: bool,
    pub suspended: bool,
    pub handoff: bool,
    pub restart_requested: bool,
    pub presentation_fault: bool,
    pub playing: bool,
    pub muted: bool,
}

impl Readiness {
    pub fn observe(engine: &AudioEngine) -> Self {
        let valid = engine.compensation_plan.applied_plan_is_valid();
        let awaiting_plan = engine.compensation_plan.awaiting_structural_plan();
        let suspended = engine.device_handoff.is_suspended();
        let handoff = engine.device_handoff.pending().is_some();
        let resident = engine.observe_resident_restart();
        let presentation_fault = engine.presentation_state.is_faulted();
        let playing = engine.transport.is_playing();
        // This quiescent diagnostic keeps its original scan before the gate's
        // owned checks, but its pure result reuses that answer instead of rescanning.
        let muted = engine.quiescent_project_processing_muted(resident);
        Self {
            valid,
            awaiting_plan,
            suspended,
            handoff,
            restart_requested: resident.is_pending(),
            presentation_fault,
            playing,
            muted,
        }
    }
}

pub(super) struct Fixture {
    pub engine: AudioEngine,
    pub commands: Producer<EngineCommand>,
    pub events: Consumer<EngineEvent>,
    pub tracks: [TrackId; 2],
    pub effect: EffectId,
    pub requested: Arc<AtomicU32>,
    pub reported: Arc<AtomicU32>,
    pub owner: Option<reconfiguration::DeviceReconfiguration>,
}

impl Fixture {
    pub fn new(start: StartingCondition) -> Self {
        let tracks = [TrackId::new(), TrackId::new()];
        let effect = EffectId::new();
        let mut delayed = routing_channel(tracks[0]);
        delayed.effects.push(routing_effect(effect));
        let (engine, mut commands, events) = graph_engine(&[
            delayed,
            routing_channel(tracks[1]),
            routing_channel(TrackId::MASTER),
        ]);
        let requested = Arc::new(AtomicU32::new(0));
        let reported = Arc::new(AtomicU32::new(0));
        let device: Box<dyn vibez_dsp::effect::AudioEffect> =
            if start == StartingCondition::InvalidPlan {
                Box::new(DelayProbe::with_shared_report(0, Arc::clone(&reported)))
            } else {
                compensation_tests::changing_delay(0, Arc::clone(&requested))
            };
        let mut affected = EngineTrack::new(tracks[0]);
        affected
            .playback_source
            .clips
            .push(constant_clip(32_768, 0.25));
        affected.effects.push(EffectSlot {
            id: effect,
            effect: device,
            bypass: false,
        });
        let mut healthy = EngineTrack::new(tracks[1]);
        healthy
            .playback_source
            .clips
            .push(constant_clip(32_768, 0.25));
        commands
            .push(EngineCommand::AddTrack(Box::new(affected)))
            .unwrap();
        commands
            .push(EngineCommand::AddTrack(Box::new(healthy)))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut fixture = Self {
            engine,
            commands,
            events,
            tracks,
            effect,
            requested,
            reported,
            owner: None,
        };
        fixture.engine.process(&mut [0.0; 128], 2);
        fixture.drain_events();
        match start {
            StartingCondition::Running => {}
            StartingCondition::AwaitingPlan => {
                fixture
                    .commands
                    .push(EngineCommand::RemoveEffect(tracks[0], effect))
                    .unwrap();
                fixture.engine.drain_commands();
            }
            StartingCondition::RestartRequested => fixture.requested.store(1, Ordering::Release),
            StartingCondition::DeviceAway => {
                fixture.requested.store(1, Ordering::Release);
                fixture.engine.begin_device_reconfiguration();
                for event in fixture.drain_events() {
                    if let EngineEvent::DeviceReconfiguration(owner) = event {
                        fixture.owner = Some(owner);
                    }
                }
                assert!(fixture.owner.is_some());
            }
            StartingCondition::InvalidPlan => {
                fixture.reported.store(1, Ordering::Release);
                fixture.engine.validate_compensation();
            }
            StartingCondition::PresentationFault => {
                while fixture
                    .engine
                    .event_tx
                    .push(EngineEvent::PlaybackPosition(0))
                    .is_ok()
                {}
                for _ in 0..=presentation_queue::PRESENTATION_EVENT_CAPACITY {
                    {
                        let mut retirement_allowance = fixture
                            .engine
                            .pending_retirements
                            .reserve(1)
                            .expect("fixture admits its original owner-return budget");
                        fixture.engine.present_event_at(
                            EngineEvent::PlaybackStarted,
                            fixture.engine.output_position + 521,
                            &mut retirement_allowance,
                        )
                    };
                }
            }
        }
        fixture.drain_events();
        fixture
    }

    pub fn plan(&self, report: u32) -> Box<crate::routing::PreparedRouting> {
        let mut affected = routing_channel(self.tracks[0]);
        if self.engine.tracks[0]
            .effects
            .iter()
            .any(|slot| slot.id == self.effect)
            || self
                .engine
                .device_handoff
                .pending()
                .is_some_and(|pending| pending.effect_id == Some(self.effect))
        {
            affected.effects.push(routing_effect(self.effect));
        }
        crate::routing::PreparedRouting::prepare_compensated(
            &[
                affected,
                vibez_core::routing::RoutingChannel {
                    effects: self.engine.tracks[1]
                        .effects
                        .iter()
                        .map(|slot| routing_effect(slot.id))
                        .collect(),
                    ..routing_channel(self.tracks[1])
                },
                routing_channel(TrackId::MASTER),
            ],
            64,
            &[(
                RoutingNode {
                    channel: self.tracks[0],
                    stage: NodeStage::Effect(self.effect),
                },
                report,
            )],
            &[],
            2,
        )
        .unwrap()
    }

    pub fn drain_events(&mut self) -> Vec<EngineEvent> {
        let mut result = Vec::new();
        loop {
            result.extend(std::iter::from_fn(|| self.events.pop().ok()));
            self.engine.flush_presentation();
            if self.events.is_empty() {
                return result;
            }
        }
    }

    pub fn drain_command(&mut self, command: EngineCommand) {
        self.commands.push(command).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| self.engine.drain_commands()),
            (0, 0)
        );
    }
}
