//! Offline rejection must follow the same detected configuration failures as playback.

use super::*;
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
    routing::{RoutingChannel, RoutingEffect},
};
use vibez_dsp::effect::AudioEffect;

struct ActivatedAt48k;
impl AudioEffect for ActivatedAt48k {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(48_000)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, output: &mut [f32], _: usize) {
        output.fill(0.75);
    }
    fn reset(&mut self) {}
}

#[test]
fn offline_rate_reuse_is_rejected_before_processing_and_retains_the_owner() {
    let id = TrackId::new();
    let effect = EffectId::new();
    let mut track = EngineTrack::new(id);
    track.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(ActivatedAt48k),
        bypass: false,
    });
    let model = [
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            }],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let mut engine = AudioEngine::for_offline_routing(OfflineRoutingSetup {
        sample_rate: 44_100,
        bpm: 120.0,
        swing: SwingAmount::STRAIGHT,
        tracks: vec![track],
        buses: vec![],
        master: EngineTrack::new(TrackId::MASTER),
        routing: crate::routing::PreparedRouting::prepare(&model, 16).unwrap(),
    });
    let mut output = [0.0; 16];
    engine.render_offline_routing_segment(0, &mut output, None);
    assert!(!engine.compensation_valid);
    let error = engine
        .offline_compensation_failure()
        .expect("invalid rate must fail Bounce");
    assert!(
        error.contains("48000") && error.contains("44100"),
        "{error}"
    );
    let (tracks, _, _) = engine.take_offline_channels();
    assert_eq!(tracks[0].effects.len(), 1);
    assert_eq!(
        tracks[0].effects[0].effect.activation_sample_rate(),
        Some(48_000)
    );
}
