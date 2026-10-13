//! Delay probes exercise running Send and detector paths through the shared executor.

use super::*;
use crate::test_support::DelayProbe;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::*;
use vibez_dsp::effect::AudioEffect;

fn channel(id: TrackId, bus: bool, effects: &[EffectId]) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: bus,
        sends: vec![],
        effects: effects
            .iter()
            .map(|&id| RoutingEffect {
                id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            })
            .collect(),
    }
}
fn pulse(track_id: TrackId) -> EngineCommand {
    let mut signal = vec![0.0; 2048];
    for sample in [0, 17, 511, 1030] {
        signal[sample] = 1.0;
    }
    EngineCommand::AddClip {
        track_id,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![signal.clone(), signal],
            sample_rate: 44100,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 2048,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}
fn render(engine: &mut AudioEngine, events: &mut rtrb::Consumer<EngineEvent>) -> Vec<f32> {
    let mut result = Vec::new();
    for frames in [1, 7, 31, 64, 137, 511].into_iter().cycle().take(24) {
        let mut output = vec![0.0; frames * 2];
        engine.process(&mut output, 2);
        result.extend(output);
        while events.pop().is_ok() {}
    }
    result
}
fn send_run(reported: u32) -> Vec<f32> {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let bus = TrackId::new();
    let insert = EffectId::new();
    let return_fx = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Pulse".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddBus(bus, "Return".into()))
        .unwrap();
    commands.push(pulse(track)).unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: insert,
            effect: Box::new(DelayProbe::new(137)),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: bus,
            effect_id: return_fx,
            effect: Box::new(DelayProbe::with_report(521, reported)),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: track,
            bus_id: bus,
            amount: 1.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackGain(bus, -1.0))
        .unwrap();
    let mut source = channel(track, false, &[insert]);
    source.sends.push(bus);
    let model = [
        source,
        channel(bus, true, &[return_fx]),
        channel(TrackId::MASTER, true, &[]),
    ];
    let reports = [
        (
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(insert),
            },
            137,
        ),
        (
            RoutingNode {
                channel: bus,
                stage: NodeStage::Effect(return_fx),
            },
            reported,
        ),
    ];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&model, 512, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    render(&mut engine, &mut events)
}
#[test]
fn delayed_send_return_cancels_direct_path_and_detects_a_wrong_return_report() {
    assert!(send_run(521).iter().all(|sample| sample.abs() < 1e-7));
    assert!(send_run(520).iter().any(|sample| sample.abs() > 0.1));
}
struct Difference {
    inputs: Vec<ExternalInputDescriptor>,
}
impl AudioEffect for Difference {
    fn external_inputs(&self) -> &[ExternalInputDescriptor] {
        &self.inputs
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
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn process_with_inputs(
        &mut self,
        main: &mut [f32],
        _: usize,
        inputs: &[ExternalInputBlock<'_>],
    ) {
        for (main, detector) in main.iter_mut().zip(inputs[0].samples) {
            *main -= detector;
        }
    }
    fn reset(&mut self) {}
}
fn detector_run(reported: u32) -> Vec<f32> {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let source = TrackId::new();
    let target = TrackId::new();
    let latency = EffectId::new();
    let detector = EffectId::new();
    let descriptor = ExternalInputDescriptor {
        id: ExternalInputId(7),
        name: "Detector".into(),
        channels: 2,
    };
    for id in [source, target] {
        commands
            .push(EngineCommand::AddTrack(id, "Pulse".into()))
            .unwrap();
        commands.push(pulse(id)).unwrap();
    }
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: source,
            effect_id: latency,
            effect: Box::new(DelayProbe::with_report(521, reported)),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: target,
            effect_id: detector,
            effect: Box::new(Difference {
                inputs: vec![descriptor.clone()],
            }),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackMute(source, true))
        .unwrap();
    let mut destination = channel(target, false, &[detector]);
    destination.effects[0].inputs.push(descriptor);
    destination.effects[0]
        .assignments
        .push(SidechainAssignment {
            input_id: ExternalInputId(7),
            input_name: "Detector".into(),
            source,
            source_name: "Pulse".into(),
            tap: SourceTap::AfterEffects,
        });
    let model = [
        destination,
        channel(source, false, &[latency]),
        channel(TrackId::MASTER, true, &[]),
    ];
    let reports = [(
        RoutingNode {
            channel: source,
            stage: NodeStage::Effect(latency),
        },
        reported,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&model, 512, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    render(&mut engine, &mut events)
}
#[test]
fn high_latency_detector_and_main_arrive_together_independent_of_source_order() {
    assert!(detector_run(521).iter().all(|sample| sample.abs() < 1e-7));
    assert!(detector_run(520).iter().any(|sample| sample.abs() > 0.1));
}
