//! Prepare explicit routing fixtures outside the callback while preserving command-driven test setup.

use super::*;
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

pub(super) fn graph_engine(
    channels: &[RoutingChannel],
) -> (AudioEngine, Producer<EngineCommand>, Consumer<EngineEvent>) {
    let (mut engine, commands, events) = AudioEngine::new();
    engine.command_set_routing(routing_plan(channels));
    (engine, commands, events)
}

pub(super) fn routing_plan(channels: &[RoutingChannel]) -> Box<crate::routing::PreparedRouting> {
    crate::routing::PreparedRouting::prepare(channels, crate::routing::MAX_BLOCK_FRAMES)
        .expect("valid graph fixture")
}

pub(super) fn routing_channel(id: TrackId) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: id.is_master(),
        effects: vec![],
        sends: vec![],
    }
}

pub(super) fn routing_effect(id: EffectId) -> RoutingEffect {
    RoutingEffect {
        id,
        inputs: vec![],
        assignments: vec![],
        inactive_inputs: vec![],
    }
}
