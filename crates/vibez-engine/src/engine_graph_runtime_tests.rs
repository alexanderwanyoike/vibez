//! Callback work must scale with blocks and bounded control intervals.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vibez_core::{
    automation::{AutomationLane, AutomationPoint, AutomationTarget},
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
    routing::{RoutingChannel, RoutingEffect},
};

struct CountingEffect {
    calls: Arc<AtomicUsize>,
    parameters: Arc<AtomicUsize>,
}
impl vibez_dsp::effect::AudioEffect for CountingEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        self.parameters.fetch_add(1, Ordering::Relaxed);
        true
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
    fn reset(&mut self) {}
}

fn setup(
    effects: usize,
    ramp: bool,
    bypass: bool,
) -> (AudioEngine, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (mut engine, _, _) = AudioEngine::new();
    engine.sample_rate = 48000;
    let id = TrackId::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let parameters = Arc::new(AtomicUsize::new(0));
    let mut track = EngineTrack::new(id);
    let mut model = RoutingChannel {
        id,
        is_bus: false,
        effects: vec![],
        sends: vec![],
    };
    for _ in 0..effects {
        let effect = EffectId::new();
        track.effects.push(EffectSlot {
            id: effect,
            effect: Box::new(CountingEffect {
                calls: Arc::clone(&calls),
                parameters: Arc::clone(&parameters),
            }),
            bypass,
        });
        model.effects.push(RoutingEffect {
            id: effect,
            inputs: vec![],
            assignments: vec![],
            inactive_inputs: vec![],
        });
    }
    let mut routing = crate::routing::PreparedRouting::prepare(
        &[
            model,
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                effects: vec![],
                sends: vec![],
            },
        ],
        512,
    )
    .unwrap();
    if ramp {
        let target = AutomationTarget::EffectParam {
            effect_id: track.effects[0].id,
            param_index: 0,
        };
        let mut lane = AutomationLane::new(target);
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.1,
            curve: 0.0,
        });
        lane.insert_point(AutomationPoint {
            beat: 1.0,
            value: 1.0,
            curve: 0.0,
        });
        track.playback_source.automation.push(lane);
        routing.configure_automation(&[(id, target)]).unwrap();
        engine.transport.play();
    }
    engine.tracks.push(track);
    engine.routing = Some(routing);
    (engine, calls, parameters)
}

#[test]
fn idle_chain_processes_each_effect_once_per_block() {
    let (mut engine, calls, _) = setup(10, false, false);
    engine.process(&mut [0.0; 1024], 2);
    assert_eq!(calls.load(Ordering::Relaxed), 10);
}

#[test]
fn ramp_automation_bounds_dsp_and_parameter_calls_per_block() {
    let (mut engine, calls, parameters) = setup(1, true, false);
    engine.process(&mut [0.0; 1024], 2);
    assert!(
        calls.load(Ordering::Relaxed) <= 8,
        "{} DSP calls",
        calls.load(Ordering::Relaxed)
    );
    assert!(parameters.load(Ordering::Relaxed) <= 8);
}

#[test]
fn graph_bypass_does_not_process_wet_audio() {
    let (mut engine, calls, _) = setup(1, false, true);
    engine.process(&mut [0.0; 1024], 2);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}
