//! Prepare project fixtures outside the callback for audio and ownership checks.

use super::*;
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::{RoutingChannel, RoutingEffect};

pub(super) fn graph_engine(
    channels: &[RoutingChannel],
) -> (AudioEngine, Producer<EngineCommand>, Consumer<EngineEvent>) {
    let (mut engine, commands, events) = AudioEngine::new();
    engine.routing = routing_plan(channels);
    engine.project_routing_published = true;
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

pub(super) fn constant_clip(frames: usize, value: f32) -> EngineClip {
    EngineClip {
        id: ClipId::new(),
        audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![value; frames]; 2],
            sample_rate: DEFAULT_SAMPLE_RATE,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: frames as u64,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}
