//! Audible Bus sums and detector-only sums share one processed signal.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::{ClipId, EffectId},
    routing::*,
};

struct BusProcessor(Arc<AtomicUsize>);
impl vibez_dsp::effect::AudioEffect for BusProcessor {
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
    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.0.fetch_add(buffer.len() / channels, Ordering::Relaxed);
        for sample in buffer {
            *sample = sample.clamp(-0.6, 0.6) * 0.5;
        }
    }
    fn reset(&mut self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

fn channel(id: TrackId) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: false,
        effects: vec![],
        sends: vec![],
    }
}
fn clip(track: TrackId, value: f32) -> EngineCommand {
    EngineCommand::AddClip {
        track_id: track,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            sample_rate: 44100,
            channels: vec![vec![value; 8192]; 2],
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 8192,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}

#[test]
fn kick_and_bass_solo_keep_the_audible_bus_return_without_ghost_snare_leakage() {
    for (kick_solo, bass_solo) in [(true, false), (false, true), (true, true)] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let kick = TrackId::new();
        let bass = TrackId::new();
        let snare = TrackId::new();
        let drums = TrackId::new();
        let gate = EffectId::new();
        let bus_fx = EffectId::new();
        for track in [kick, bass, snare] {
            commands
                .push(EngineCommand::AddTrack(track, "Track".into()))
                .unwrap();
        }
        commands
            .push(EngineCommand::AddBus(drums, "Drums".into()))
            .unwrap();
        for (track, value) in [(kick, 0.05), (bass, 0.2), (snare, 1.0)] {
            commands.push(clip(track, value)).unwrap();
        }
        for track in [kick, snare] {
            commands
                .push(EngineCommand::SetSend {
                    track_id: track,
                    bus_id: drums,
                    amount: 1.0,
                })
                .unwrap();
        }
        commands
            .push(EngineCommand::SetTrackSolo(kick, kick_solo))
            .unwrap();
        commands
            .push(EngineCommand::SetTrackSolo(bass, bass_solo))
            .unwrap();
        commands
            .push(EngineCommand::AddEffect {
                track_id: bass,
                effect_id: gate,
                effect_type: EffectType::Gate,
                position: None,
            })
            .unwrap();
        commands
            .push(EngineCommand::SetEffectParam {
                track_id: bass,
                effect_id: gate,
                param_index: 0,
                value: -20.0,
            })
            .unwrap();
        let processed = Arc::new(AtomicUsize::new(0));
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: drums,
                effect_id: bus_fx,
                effect: Box::new(BusProcessor(Arc::clone(&processed))),
                position: None,
            })
            .unwrap();
        let mut bass_model = channel(bass);
        bass_model.effects.push(RoutingEffect {
            id: gate,
            inputs: vec![ExternalInputDescriptor {
                id: ExternalInputId(0),
                name: "Sidechain".into(),
                channels: 2,
            }],
            assignments: vec![SidechainAssignment {
                input_id: ExternalInputId(0),
                input_name: "Sidechain".into(),
                source: drums,
                source_name: "Drums".into(),
                tap: SourceTap::AfterEffects,
            }],
            inactive_inputs: vec![],
        });
        let mut drums_model = channel(drums);
        drums_model.is_bus = true;
        drums_model.effects.push(RoutingEffect {
            id: bus_fx,
            inputs: vec![],
            assignments: vec![],
            inactive_inputs: vec![],
        });
        let mut kick_model = channel(kick);
        kick_model.sends.push(drums);
        let mut snare_model = channel(snare);
        snare_model.sends.push(drums);
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(
                    &[
                        kick_model,
                        bass_model,
                        snare_model,
                        drums_model,
                        channel(TrackId::MASTER),
                    ],
                    64,
                )
                .unwrap(),
            ))
            .unwrap();
        engine.process(&mut [], 2);
        while events.pop().is_ok() {}
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 8192];
        let mut captured = [0.0; 8192];
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2)
                .with_track_output_capture(drums.raw(), &mut captured),
        );
        let pan = std::f32::consts::FRAC_1_SQRT_2;
        let expected_bus = if kick_solo { 0.05 * pan * 0.5 } else { 0.0 };
        let expected_mix = if kick_solo {
            0.05 * pan + expected_bus
        } else {
            0.2 * pan
        };
        for sample in &output[output.len() - 128..] {
            assert!(
                (*sample - expected_mix).abs() < 1e-5,
                "solo {kick_solo}/{bass_solo}: {sample} != {expected_mix}"
            );
        }
        for sample in &captured[captured.len() - 128..] {
            assert!(
                (*sample - expected_bus).abs() < 1e-5,
                "capture {sample} != {expected_bus}"
            );
        }
        assert_eq!(
            processed.load(Ordering::Relaxed),
            4096,
            "one Bus state must process each frame once"
        );
        while events.pop().is_ok() {}
        let mut output = [0.0; 128];
        let mut captured = [0.0; 128];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process_block(
                AudioProcessBlock::new(&mut output, 2)
                    .with_track_output_capture(drums.raw(), &mut captured)
            )),
            (0, 0)
        );
        assert_eq!(engine.tracks[0].solo, kick_solo);
        assert_eq!(engine.tracks[1].solo, bass_solo);
        assert_eq!(engine.tracks[2].sends, [(drums, 1.0)]);
        assert!(!engine.buses[0].solo);
    }
}

#[test]
fn send_levels_cross_zero_without_replacing_the_prepared_graph() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let source = TrackId::new();
    let bus = TrackId::new();
    commands
        .push(EngineCommand::AddTrack(source, "Source".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddBus(bus, "Bus".into()))
        .unwrap();
    commands.push(clip(source, 1.0)).unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: source,
            bus_id: bus,
            amount: 0.0,
        })
        .unwrap();
    let mut source_model = channel(source);
    source_model.sends.push(bus);
    let mut bus_model = channel(bus);
    bus_model.is_bus = true;
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[source_model, bus_model, channel(TrackId::MASTER)],
                64,
            )
            .unwrap(),
        ))
        .unwrap();
    engine.process(&mut [], 2);
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 128];
    engine.process(&mut output, 2);
    let identity = std::ptr::from_ref(engine.routing.as_deref().unwrap());
    let edges = engine.routing.as_ref().unwrap().graph.edges.clone();
    for amount in [0.5, 0.0, 1.0, 0.0] {
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::SetSend {
                track_id: source,
                bus_id: bus,
                amount,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
            (0, 0)
        );
        assert_eq!(
            std::ptr::from_ref(engine.routing.as_deref().unwrap()),
            identity
        );
        assert_eq!(engine.routing.as_ref().unwrap().graph.edges, edges);
        let expected = (1.0 + amount) * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
    }
}

#[test]
fn ordinary_bus_delay_tail_survives_solo_without_a_selected_track_send() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let source = TrackId::new();
    let selected = TrackId::new();
    let bus = TrackId::new();
    let effect = EffectId::new();
    for track in [source, selected] {
        commands
            .push(EngineCommand::AddTrack(track, "Track".into()))
            .unwrap();
    }
    commands
        .push(EngineCommand::AddBus(bus, "Delay return".into()))
        .unwrap();
    let mut pulse = clip(source, 0.0);
    if let EngineCommand::AddClip { audio, .. } = &mut pulse {
        for channel in &mut Arc::get_mut(audio).unwrap().channels {
            channel[0] = 1.0;
        }
    }
    commands.push(pulse).unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: source,
            bus_id: bus,
            amount: 1.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: bus,
            effect_id: effect,
            effect_type: EffectType::Delay,
            position: None,
        })
        .unwrap();
    for (param_index, value) in [(0, 1.0), (1, 0.5), (2, 1.0)] {
        commands
            .push(EngineCommand::SetEffectParam {
                track_id: bus,
                effect_id: effect,
                param_index,
                value,
            })
            .unwrap();
    }
    let mut source_model = channel(source);
    source_model.sends.push(bus);
    let mut bus_model = channel(bus);
    bus_model.is_bus = true;
    bus_model.effects.push(RoutingEffect {
        id: effect,
        inputs: vec![],
        assignments: vec![],
        inactive_inputs: vec![],
    });
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[
                    source_model,
                    channel(selected),
                    bus_model,
                    channel(TrackId::MASTER),
                ],
                64,
            )
            .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 64], 2);
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetTrackSolo(selected, true))
        .unwrap();
    let mut output = [0.0; 128];
    let mut capture = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process_block(
            AudioProcessBlock::new(&mut output, 2)
                .with_track_output_capture(bus.raw(), &mut capture)
        )),
        (0, 0)
    );
    assert!(
        output[12 * 2] > 0.7,
        "44-frame echo was cut off by unrelated track solo"
    );
    assert_eq!(output, capture, "the audible return must remain capturable");
    assert!(engine.tracks[1].sends.is_empty());
}
