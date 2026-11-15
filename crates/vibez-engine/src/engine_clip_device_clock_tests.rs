//! Devices hear the Perform clock, not the Clip clock, during clip performance.

use super::*;
use crate::playback_source::PreparedClipPlayback;
use graph_test_support::{routing_channel, routing_effect};
use std::sync::atomic::{AtomicU64, Ordering};
use vibez_core::audio_context::DeviceAudioContext;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::midi::InstrumentKind;
use vibez_core::perform::MusicalBoundary;
use vibez_dsp::effect::AudioEffect;

const BAR: u64 = 16;
const CALLBACK: usize = 4;

#[derive(Clone, Default)]
struct HeardPosition(Arc<AtomicU64>);

impl HeardPosition {
    fn hear(&self, context: DeviceAudioContext) {
        self.0.store(context.musical_sample, Ordering::Relaxed);
    }
    fn last(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

struct InstrumentProbe(HeardPosition);
impl vibez_instruments::Instrument for InstrumentProbe {
    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        self.0.hear(context);
    }
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
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
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

struct EffectProbe(HeardPosition);
impl AudioEffect for EffectProbe {
    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        self.0.hear(context);
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
    fn reset(&mut self) {}
}

struct DeviceProbes {
    instrument: HeardPosition,
    effect: HeardPosition,
}

/// One looping bar on `clipped`; `bare` plays no Clip.
fn perform_one_bar_clip() -> (AudioEngine, rtrb::Consumer<EngineEvent>, [DeviceProbes; 2]) {
    let (engine, mut commands, events) = AudioEngine::new();
    let clipped = TrackId::new();
    let bare = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    let mut model = Vec::new();
    let probes = [clipped, bare].map(|track| {
        let probes = DeviceProbes {
            instrument: HeardPosition::default(),
            effect: HeardPosition::default(),
        };
        let effect = EffectId::new();
        for command in [
            EngineCommand::add_track(track),
            EngineCommand::SetInstrument {
                track_id: track,
                instrument: Box::new(InstrumentProbe(probes.instrument.clone())),
            },
            EngineCommand::InsertEffect {
                track_id: track,
                effect_id: effect,
                effect: Box::new(EffectProbe(probes.effect.clone())),
                position: None,
            },
        ] {
            commands.push(command).unwrap();
        }
        let mut channel = routing_channel(track);
        channel.effects.push(routing_effect(effect));
        model.push(channel);
        probes
    });
    model.push(routing_channel(TrackId::MASTER));
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 64).unwrap(),
        ))
        .unwrap();
    let clip_id = ClipId::new();
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![Box::new(PreparedClipPlayback {
                track_id: clipped,
                clip_id: Some(clip_id),
                request_id: 1,
                length_samples: BAR,
                looping: true,
                launch_mode: Default::default(),
                start_position: 0,
                source: Box::default(),
            })],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    (engine, events, probes)
}

/// The position each device heard at the start of every callback.
fn heard_over_three_loops(
    engine: &mut AudioEngine,
    events: &mut rtrb::Consumer<EngineEvent>,
    probes: &DeviceProbes,
) -> (Vec<u64>, Vec<u64>) {
    let mut instrument = Vec::new();
    let mut effect = Vec::new();
    for _ in 0..3 * BAR as usize / CALLBACK {
        engine.process(&mut [0.0; CALLBACK * 2], 2);
        instrument.push(probes.instrument.last());
        effect.push(probes.effect.last());
        while events.pop().is_ok() {}
    }
    (instrument, effect)
}

fn perform_clock() -> Vec<u64> {
    (0..3 * BAR).step_by(CALLBACK).collect()
}

#[test]
fn devices_on_a_looping_clip_keep_counting_past_the_clip_length() {
    let (mut engine, mut events, [clipped, _]) = perform_one_bar_clip();
    let (instrument, effect) = heard_over_three_loops(&mut engine, &mut events, &clipped);
    assert!(engine.clip_performance);
    assert_eq!(instrument, perform_clock(), "instrument");
    assert_eq!(effect, perform_clock(), "effect");
}

#[test]
fn devices_on_a_track_without_a_clip_follow_the_perform_clock() {
    let (mut engine, mut events, [_, bare]) = perform_one_bar_clip();
    let (instrument, effect) = heard_over_three_loops(&mut engine, &mut events, &bare);
    assert!(engine.clip_performance);
    assert_eq!(instrument, perform_clock(), "instrument");
    assert_eq!(effect, perform_clock(), "effect");
}
