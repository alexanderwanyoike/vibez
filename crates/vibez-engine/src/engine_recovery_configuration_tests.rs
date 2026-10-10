//! Configuration rejection closes Capture at the source callback boundary.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
    routing::{RoutingChannel, RoutingEffect},
};

struct ConfigurationProbe(Arc<AtomicBool>);
impl vibez_dsp::effect::AudioEffect for ConfigurationProbe {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(48000)
    }
    fn processing_configuration_valid(&self) -> bool {
        self.0.load(Ordering::Relaxed)
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
    fn process(&mut self, _: &mut [f32], _: usize) {
        assert!(self.0.load(Ordering::Relaxed));
    }
    fn reset(&mut self) {}
}

fn setup() -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    Arc<AtomicBool>,
    EffectId,
) {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let valid = Arc::new(AtomicBool::new(true));
    commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
    commands
        .push(EngineCommand::AddTrack(track, "Probe".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(ConfigurationProbe(Arc::clone(&valid))),
            position: None,
        })
        .unwrap();
    let model = [
        RoutingChannel {
            id: track,
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
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 16).unwrap(),
        ))
        .unwrap();
    engine.process(&mut [], 2);
    while events.pop().is_ok() {}
    engine.transport.seek(500);
    (engine, commands, events, valid, effect)
}

#[test]
fn active_configuration_or_rate_failure_closes_capture_at_actual_source_time() {
    for wrong_rate in [false, true] {
        let (mut engine, _, mut events, valid, effect) = setup();
        if wrong_rate {
            engine.sample_rate = 44100;
        } else {
            valid.store(false, Ordering::Relaxed);
        }
        let mut output = [1.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        assert_eq!(output, [0.0; 16]);
        let collected: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
        assert!(collected.iter().any(|event| matches!(
            event,
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples: 500
            }
        )));
        assert!(collected.iter().any(|event| matches!(event, EngineEvent::CompensationInvalid {effect_id: Some(id), ..} if *id == effect)));
        assert!(!engine.compensation_valid);
    }
}

#[test]
fn full_event_ring_retains_source_capture_stop_and_named_failure_before_metering() {
    let (mut engine, _, mut events, valid, effect) = setup();
    while engine.event_tx.push(EngineEvent::PlaybackStopped).is_ok() {}
    valid.store(false, Ordering::Relaxed);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 2)),
        (0, 0)
    );
    assert_eq!(engine.pending_capture_stop, Some(500));
    assert_eq!(engine.pending_compensation_failure.unwrap().1, Some(effect));
    while events.pop().is_ok() {}
    engine.process(&mut [0.0; 16], 2);
    assert!(matches!(
        events.pop(),
        Ok(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: 500
        })
    ));
    assert!(
        matches!(events.pop(), Ok(EngineEvent::CompensationInvalid {effect_id: Some(id), ..}) if id == effect)
    );
}

#[test]
fn invalid_configuration_cannot_start_a_new_silent_capture() {
    let (mut engine, mut commands, mut events, valid, _) = setup();
    valid.store(false, Ordering::Relaxed);
    engine.process(&mut [0.0; 16], 2);
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 16], 2);
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::PerformanceCaptureStarted { .. })));
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::CompensationInvalid {
            reason: "Capture requires a valid applied audio configuration",
            ..
        }
    )));
}
