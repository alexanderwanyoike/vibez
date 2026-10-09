mod support;
use vibez_core::{
    audio_buffer::DecodedAudio,
    id::{ClipId, EffectId, TrackId},
    routing::*,
};
use vibez_engine::{commands::EngineCommand, engine::AudioEngine, routing::PreparedRouting};
use vibez_plugin_host::wrappers::effect::PluginEffectWrapper;

fn clip(track_id: TrackId, left: f32, right: f32) -> EngineCommand {
    EngineCommand::AddClip {
        track_id,
        clip_id: ClipId::new(),
        audio: std::sync::Arc::new(DecodedAudio {
            channels: vec![vec![left; 128], vec![right; 128]],
            sample_rate: 48000,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 128,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
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

#[test]
fn loadable_formats_deliver_declared_inputs_at_short_and_variable_blocks() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut plugin = fixture.load(format, 64);
        let inputs = plugin.external_inputs().to_vec();
        assert_eq!(
            inputs
                .iter()
                .map(|input| input.channels)
                .collect::<Vec<_>>(),
            vec![1, 2, 6]
        );
        for frames in [1, 7, 31, 64] {
            let mut main = vec![0.1; frames * 2];
            let mono = vec![0.25; frames];
            let stereo: Vec<_> = (0..frames).flat_map(|_| [0.2, 0.4]).collect();
            plugin.process_with_inputs(
                &mut main,
                2,
                &[
                    ExternalInputBlock {
                        id: inputs[0].id,
                        channels: 1,
                        samples: &mono,
                        connected: true,
                    },
                    ExternalInputBlock {
                        id: inputs[1].id,
                        channels: 2,
                        samples: &stereo,
                        connected: true,
                    },
                ],
            );
            for frame in main.chunks_exact(2) {
                assert!((frame[0] - 1.2).abs() < 1e-6);
                assert!((frame[1] - 1.8).abs() < 1e-6);
            }
            let mut disconnected = vec![0.1; frames * 2];
            plugin.process_audio(&mut disconnected, 2);
            assert!(disconnected
                .iter()
                .all(|sample| (*sample - 0.1).abs() < 1e-6));
        }
        plugin.stop_processing();
        plugin.deactivate();
    }
}

#[test]
fn production_engine_routes_two_independent_plugin_inputs_without_audible_sources() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let plugin = fixture.load(format, 64);
        let inputs = plugin.external_inputs().to_vec();
        let source_mono = TrackId::new();
        let source_stereo = TrackId::new();
        let receiver = TrackId::new();
        let effect = EffectId::new();
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        for track in [receiver, source_stereo, source_mono] {
            commands
                .push(EngineCommand::AddTrack(track, "Fixture".into()))
                .unwrap();
        }
        for (track, left, right) in [
            (receiver, 0.1, 0.1),
            (source_mono, 0.5, 0.0),
            (source_stereo, 0.2, 0.4),
        ] {
            commands.push(clip(track, left, right)).unwrap();
        }
        for source in [source_mono, source_stereo] {
            commands
                .push(EngineCommand::SetTrackMute(source, true))
                .unwrap();
        }
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: receiver,
                effect_id: effect,
                effect: Box::new(PluginEffectWrapper::new(plugin)),
                position: None,
            })
            .unwrap();
        let mut receiving = channel(receiver);
        receiving.effects.push(RoutingEffect {
            id: effect,
            inputs: inputs.clone(),
            assignments: vec![
                SidechainAssignment {
                    input_id: inputs[0].id,
                    input_name: inputs[0].name.clone(),
                    source: source_mono,
                    source_name: "Mono".into(),
                    tap: SourceTap::AfterEffects,
                },
                SidechainAssignment {
                    input_id: inputs[1].id,
                    input_name: inputs[1].name.clone(),
                    source: source_stereo,
                    source_name: "Stereo".into(),
                    tap: SourceTap::BeforeEffects,
                },
            ],
        });
        commands
            .push(EngineCommand::SetRouting(
                PreparedRouting::prepare(
                    &[
                        receiving,
                        channel(source_stereo),
                        channel(source_mono),
                        channel(TrackId::MASTER),
                    ],
                    64,
                )
                .unwrap(),
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        for frames in [1, 7, 31, 64] {
            let mut output = vec![0.0; frames * 2];
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
            for frame in output.chunks_exact(2) {
                assert!(
                    (frame[0] - 1.2 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
                    "{format}: {}",
                    frame[0]
                );
                assert!((frame[1] - 1.8 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
            }
        }
        let delivered: Vec<_> = std::iter::from_fn(|| events.pop().ok())
            .filter_map(|event| match event {
                vibez_engine::events::EngineEvent::SidechainInputMeter {
                    input_id,
                    peak_l,
                    peak_r,
                    ..
                } => Some((input_id, peak_l, peak_r)),
                _ => None,
            })
            .collect();
        assert!(delivered.contains(&(inputs[0].id, 0.25, 0.25)));
        assert!(delivered.contains(&(inputs[1].id, 0.2, 0.4)));
        drop(engine);
    }
}
