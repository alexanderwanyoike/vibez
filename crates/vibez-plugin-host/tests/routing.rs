mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;
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
            let allocations = support::allocation::count_allocations(|| {
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
                )
            });
            assert_eq!(allocations, 0, "{format} input delivery allocated");
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
        let (mut engine, mut commands, _events) = AudioEngine::new();
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
        drop(engine);
    }
}

#[test]
fn loaded_instrument_formats_trigger_other_formats_on_the_exact_note_frames() {
    use vibez_plugin_host::wrappers::instrument::PluginInstrumentWrapper;
    let fixture = support::Fixture::new();
    for source_format in ["clap", "vst3"] {
        for receiving_format in ["clap", "vst3"] {
            let source = fixture.load_instrument(source_format, 64);
            let receiving = fixture.load(receiving_format, 64);
            let input = receiving.external_inputs()[0].clone();
            let ghost = TrackId::new();
            let bass = TrackId::new();
            let effect = EffectId::new();
            let note_clip = ClipId::new();
            let (mut engine, mut commands, _events) = AudioEngine::new();
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            commands
                .push(EngineCommand::AddTrack(bass, "Bass".into()))
                .unwrap();
            commands
                .push(EngineCommand::AddMidiTrack(ghost, "Pulse".into()))
                .unwrap();
            commands.push(clip(bass, 0.1, 0.1)).unwrap();
            commands
                .push(EngineCommand::SetPluginInstrument {
                    track_id: ghost,
                    instrument: Box::new(PluginInstrumentWrapper::new(source)),
                })
                .unwrap();
            commands
                .push(EngineCommand::AddNoteClip {
                    track_id: ghost,
                    clip_id: note_clip,
                    position_beats: 0.0,
                    duration_beats: 1.0,
                    start_marker_beats: 0.0,
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                    groove_grid: Default::default(),
                })
                .unwrap();
            commands
                .push(EngineCommand::AddNote {
                    track_id: ghost,
                    clip_id: note_clip,
                    note: vibez_core::midi::MidiNote {
                        pitch: 60,
                        velocity: 100,
                        start_beat: 3.0 / 24000.0,
                        duration_beats: 4.0 / 24000.0,
                    },
                })
                .unwrap();
            commands
                .push(EngineCommand::SetTrackMute(ghost, true))
                .unwrap();
            commands
                .push(EngineCommand::AddPluginEffect {
                    track_id: bass,
                    effect_id: effect,
                    effect: Box::new(PluginEffectWrapper::new(receiving)),
                    position: None,
                })
                .unwrap();
            let mut bass_model = channel(bass);
            bass_model.effects.push(RoutingEffect {
                id: effect,
                inputs: vec![input.clone()],
                assignments: vec![SidechainAssignment {
                    input_id: input.id,
                    input_name: input.name,
                    source: ghost,
                    source_name: "Pulse".into(),
                    tap: SourceTap::BeforeEffects,
                }],
            });
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare(
                        &[bass_model, channel(ghost), channel(TrackId::MASTER)],
                        64,
                    )
                    .unwrap(),
                ))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            let mut output = [0.0; 34];
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
            for (frame, samples) in output.chunks_exact(2).enumerate() {
                let expected = if (3..7).contains(&frame) { 1.6 } else { 0.1 };
                for sample in samples {
                    assert!(
                        (*sample - expected * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
                        "{source_format} -> {receiving_format}, frame {frame}: {sample}"
                    );
                }
            }
        }
    }
}

#[test]
fn all_instrument_source_formats_feed_builtins_and_hosted_receivers() {
    use vibez_core::{effect::EffectType, midi::InstrumentKind};
    use vibez_plugin_host::wrappers::instrument::PluginInstrumentWrapper;
    let fixture = support::Fixture::new();
    for source_format in ["builtin", "clap", "vst3"] {
        for receiver_format in ["gate", "compressor", "clap", "vst3"] {
            let ghost = TrackId::new();
            let bass = TrackId::new();
            let effect = EffectId::new();
            let note_clip = ClipId::new();
            let (mut engine, mut commands, _events) = AudioEngine::new();
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            commands
                .push(EngineCommand::AddTrack(bass, "Bass".into()))
                .unwrap();
            let mut main = clip(bass, 0.01, 0.01);
            if let EngineCommand::AddClip {
                audio, duration, ..
            } = &mut main
            {
                *duration = 4096;
                *audio = std::sync::Arc::new(DecodedAudio {
                    channels: vec![vec![0.01; 4096], vec![0.01; 4096]],
                    sample_rate: 48000,
                });
            }
            commands.push(main).unwrap();
            if source_format == "builtin" {
                commands
                    .push(EngineCommand::AddInstrumentTrack(
                        ghost,
                        "Sampler".into(),
                        InstrumentKind::Sampler,
                    ))
                    .unwrap();
                commands
                    .push(EngineCommand::LoadSamplerSample {
                        track_id: ghost,
                        sample: std::sync::Arc::new(DecodedAudio {
                            channels: vec![vec![0.75; 8192]],
                            sample_rate: 48000,
                        }),
                        sample_name: "Constant probe".into(),
                    })
                    .unwrap();
            } else {
                commands
                    .push(EngineCommand::AddMidiTrack(ghost, "Instrument".into()))
                    .unwrap();
                commands
                    .push(EngineCommand::SetPluginInstrument {
                        track_id: ghost,
                        instrument: Box::new(PluginInstrumentWrapper::new(
                            fixture.load_instrument(source_format, 4096),
                        )),
                    })
                    .unwrap();
            }
            commands
                .push(EngineCommand::AddNoteClip {
                    track_id: ghost,
                    clip_id: note_clip,
                    position_beats: 0.0,
                    duration_beats: 1.0,
                    start_marker_beats: 0.0,
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                    groove_grid: Default::default(),
                })
                .unwrap();
            commands
                .push(EngineCommand::AddNote {
                    track_id: ghost,
                    clip_id: note_clip,
                    note: vibez_core::midi::MidiNote {
                        pitch: 60,
                        velocity: 127,
                        start_beat: 0.0,
                        duration_beats: 1.0,
                    },
                })
                .unwrap();
            commands
                .push(EngineCommand::SetTrackMute(ghost, true))
                .unwrap();
            commands
                .push(EngineCommand::SetTrackSolo(bass, true))
                .unwrap();
            let inputs = if matches!(receiver_format, "gate" | "compressor") {
                let kind = if receiver_format == "gate" {
                    EffectType::Gate
                } else {
                    EffectType::Compressor
                };
                let device = vibez_dsp::factory::create_effect(kind, 48000.0);
                let inputs = device.external_inputs().to_vec();
                commands
                    .push(EngineCommand::AddEffect {
                        track_id: bass,
                        effect_id: effect,
                        effect_type: kind,
                        position: None,
                    })
                    .unwrap();
                commands
                    .push(EngineCommand::SetEffectParam {
                        track_id: bass,
                        effect_id: effect,
                        param_index: 0,
                        value: -20.0,
                    })
                    .unwrap();
                inputs
            } else {
                let device = fixture.load(receiver_format, 4096);
                let inputs = device.external_inputs().to_vec();
                commands
                    .push(EngineCommand::AddPluginEffect {
                        track_id: bass,
                        effect_id: effect,
                        effect: Box::new(PluginEffectWrapper::new(device)),
                        position: None,
                    })
                    .unwrap();
                inputs
            };
            let input = &inputs[0];
            let mut bass_model = channel(bass);
            bass_model.effects.push(RoutingEffect {
                id: effect,
                inputs: inputs.clone(),
                assignments: vec![SidechainAssignment {
                    input_id: input.id,
                    input_name: input.name.clone(),
                    source: ghost,
                    source_name: "Instrument".into(),
                    tap: SourceTap::BeforeEffects,
                }],
            });
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare(
                        &[bass_model, channel(ghost), channel(TrackId::MASTER)],
                        4096,
                    )
                    .unwrap(),
                ))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            let mut output = vec![0.0; 4096];
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
            let tail = output[output.len() - 128..]
                .iter()
                .map(|sample| sample.abs())
                .sum::<f32>()
                / 128.0;
            match receiver_format {
                "gate" => assert!(tail > 0.006 && tail < 0.008),
                "compressor" => assert!(tail > 0.0 && tail < 0.005),
                _ => assert!(tail > 0.2),
            }
        }
    }
}

#[test]
fn source_taps_apply_effects_fader_and_pan_only_at_the_requested_stage() {
    use vibez_core::effect::EffectType;
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        for tap in [
            SourceTap::BeforeEffects,
            SourceTap::AfterEffects,
            SourceTap::AfterFader,
        ] {
            let plugin = fixture.load(format, 64);
            let input = plugin.external_inputs()[0].clone();
            let ghost = TrackId::new();
            let bass = TrackId::new();
            let effect = EffectId::new();
            let gain_effect = EffectId::new();
            let (mut engine, mut commands, _events) = AudioEngine::new();
            for track in [bass, ghost] {
                commands
                    .push(EngineCommand::AddTrack(track, "Track".into()))
                    .unwrap();
            }
            commands.push(clip(bass, 0.1, 0.1)).unwrap();
            commands.push(clip(ghost, 1.0, 1.0)).unwrap();
            commands
                .push(EngineCommand::SetTrackGain(ghost, 0.25))
                .unwrap();
            commands
                .push(EngineCommand::SetTrackPan(ghost, 0.0))
                .unwrap();
            commands
                .push(EngineCommand::SetTrackSolo(bass, true))
                .unwrap();
            commands
                .push(EngineCommand::AddEffect {
                    track_id: ghost,
                    effect_id: gain_effect,
                    effect_type: EffectType::Gain,
                    position: None,
                })
                .unwrap();
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id: ghost,
                    effect_id: gain_effect,
                    param_index: 0,
                    value: 0.5,
                })
                .unwrap();
            commands
                .push(EngineCommand::AddPluginEffect {
                    track_id: bass,
                    effect_id: effect,
                    effect: Box::new(PluginEffectWrapper::new(plugin)),
                    position: None,
                })
                .unwrap();
            let mut source = channel(ghost);
            source.effects.push(RoutingEffect {
                id: gain_effect,
                inputs: vec![],
                assignments: vec![],
            });
            let mut receiver = channel(bass);
            receiver.effects.push(RoutingEffect {
                id: effect,
                inputs: vec![input.clone()],
                assignments: vec![SidechainAssignment {
                    input_id: input.id,
                    input_name: input.name,
                    source: ghost,
                    source_name: "Ghost".into(),
                    tap,
                }],
            });
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare(&[receiver, source, channel(TrackId::MASTER)], 64)
                        .unwrap(),
                ))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            let mut output = [0.0; 34];
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut output, 2));
            let trigger = match tap {
                SourceTap::BeforeEffects => 1.0,
                SourceTap::AfterEffects => 0.5,
                SourceTap::AfterFader => 0.0625,
            };
            let expected = (0.1 + 2.0 * trigger) * std::f32::consts::FRAC_1_SQRT_2;
            assert!(output
                .iter()
                .all(|sample| (*sample - expected).abs() < 1e-6));
        }
    }
}

#[test]
fn same_pitch_release_precedes_retrigger_in_mixed_timestamp_plugin_events() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut plugin = fixture.load_instrument(format, 64);
        plugin.note_on_at(60, 100, 0);
        plugin.note_on_at(60, 100, 5);
        plugin.note_off_at(60, 9);
        plugin.note_off_at(60, 5);
        let mut output = [0.0; 32];
        let allocations =
            support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2));
        assert_eq!(allocations, 0);
        for (frame, channels) in output.chunks_exact(2).enumerate() {
            assert_eq!(
                channels,
                &[if frame < 9 { 0.75 } else { 0.0 }; 2],
                "{format} frame{frame}"
            );
        }
        plugin.stop_processing();
        plugin.deactivate();
    }
}
