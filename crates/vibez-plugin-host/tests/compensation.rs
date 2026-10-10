mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::routing::*;
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};
use vibez_engine::routing::PreparedRouting;
use vibez_plugin_host::{PluginEffectWrapper, PluginInstance};

fn delay(
    fixture: &support::Fixture,
    format: &str,
    frames: u32,
    actual: u32,
    report: u32,
) -> Box<dyn PluginInstance> {
    let mut plugin = fixture.load(format, frames);
    let state: Vec<u8> = [actual, report, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(plugin.load_state(&state));
    plugin.reconfigure_on_main_thread().unwrap();
    plugin
}

fn channel(id: TrackId, effects: &[EffectId]) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: false,
        sends: vec![],
        effects: effects
            .iter()
            .map(|&id| RoutingEffect {
                inactive_inputs: vec![],
                id,
                inputs: vec![],
                assignments: vec![],
            })
            .collect(),
    }
}

fn pulses(track_id: TrackId, amplitude: f32) -> EngineCommand {
    let mut samples = vec![0.0; 4096];
    for onset in [0, 17, 127, 513, 1079] {
        samples[onset] = amplitude;
    }
    EngineCommand::AddClip {
        track_id,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![samples.clone(), samples],
            sample_rate: 48000,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 4096,
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
fn mixed_loadable_effects_cancel_after_exact_parallel_compensation() {
    let fixture = support::Fixture::new();
    for formats in [["clap", "vst3"], ["vst3", "clap"]] {
        for wrong in [false, true] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let tracks = [TrackId::new(), TrackId::new()];
            let effects = [EffectId::new(), EffectId::new()];
            let reports = [137, if wrong { 522 } else { 521 }];
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            for index in 0..2 {
                commands
                    .push(EngineCommand::AddTrack(tracks[index], "Pulse".into()))
                    .unwrap();
                commands
                    .push(pulses(tracks[index], if index == 0 { 1.0 } else { -1.0 }))
                    .unwrap();
                commands
                    .push(EngineCommand::AddPluginEffect {
                        track_id: tracks[index],
                        effect_id: effects[index],
                        effect: Box::new(PluginEffectWrapper::new(delay(
                            &fixture,
                            formats[index],
                            64,
                            [137, 521][index],
                            reports[index],
                        ))),
                        position: None,
                    })
                    .unwrap();
            }
            let channels = [
                channel(tracks[1], &effects[1..]),
                channel(tracks[0], &effects[..1]),
                channel(TrackId::MASTER, &[]),
            ];
            let reports: Vec<_> = (0..2)
                .map(|index| {
                    (
                        RoutingNode {
                            channel: tracks[index],
                            stage: NodeStage::Effect(effects[index]),
                        },
                        reports[index],
                    )
                })
                .collect();
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1).unwrap(),
                ))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            let mut warmup = [0.0; 2];
            engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
            let mut peak = 0.0f32;
            for frames in [1, 7, 31, 64, 513].into_iter().cycle().take(20) {
                let mut output = vec![0.0; frames * 2];
                let allocations = support::allocation::count_allocations(|| {
                    engine.process_block(AudioProcessBlock::new(&mut output, 2))
                });
                assert_eq!(allocations, 0);
                peak = output
                    .iter()
                    .fold(peak, |peak, sample| peak.max(sample.abs()));
                while events.pop().is_ok() {}
            }
            if wrong {
                assert!(peak > 0.1);
            } else {
                assert_eq!(peak, 0.0);
            }
        }
    }
}

#[test]
fn loaded_instrument_latency_joins_the_same_compensated_mix() {
    let fixture = support::Fixture::new();
    for instrument_format in ["clap", "vst3"] {
        for effect_format in ["clap", "vst3"] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let instrument_track = TrackId::new();
            let audio_track = TrackId::new();
            let effect = EffectId::new();
            let mut instrument = fixture.load_instrument(instrument_format, 64);
            let state: Vec<_> = [137u32, 137, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert!(instrument.load_state(&state));
            instrument.reconfigure_on_main_thread().unwrap();
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            for id in [audio_track, instrument_track] {
                commands
                    .push(EngineCommand::AddTrack(id, "Controlled".into()))
                    .unwrap();
            }
            commands
                .push(EngineCommand::SetPluginInstrument {
                    track_id: instrument_track,
                    instrument: Box::new(vibez_plugin_host::PluginInstrumentWrapper::new(
                        instrument,
                    )),
                })
                .unwrap();
            let mut audio = pulses(audio_track, -0.75);
            if let EngineCommand::AddClip { audio, .. } = &mut audio {
                *audio = Arc::new(DecodedAudio {
                    channels: vec![vec![-0.75; 4096]; 2],
                    sample_rate: 48000,
                });
            }
            commands.push(audio).unwrap();
            commands
                .push(EngineCommand::AddPluginEffect {
                    track_id: audio_track,
                    effect_id: effect,
                    effect: Box::new(PluginEffectWrapper::new(delay(
                        &fixture,
                        effect_format,
                        64,
                        521,
                        521,
                    ))),
                    position: None,
                })
                .unwrap();
            commands
                .push(EngineCommand::ExternalNoteOn {
                    track_id: instrument_track,
                    pitch: 60,
                    velocity: 100,
                })
                .unwrap();
            let channels = [
                channel(audio_track, &[effect]),
                channel(instrument_track, &[]),
                channel(TrackId::MASTER, &[]),
            ];
            let reports = [
                (
                    RoutingNode {
                        channel: audio_track,
                        stage: NodeStage::Effect(effect),
                    },
                    521,
                ),
                (
                    RoutingNode {
                        channel: instrument_track,
                        stage: NodeStage::Source,
                    },
                    137,
                ),
            ];
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1).unwrap(),
                ))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            for _ in 0..32 {
                let mut output = [0.0; 128];
                engine.process_block(AudioProcessBlock::new(&mut output, 2));
                assert!(
                    output.iter().all(|sample| sample.abs() < 1e-7),
                    "{instrument_format} / {effect_format}"
                );
                while events.pop().is_ok() {}
            }
        }
    }
}

#[test]
fn delayed_loadable_one_shot_notes_release_at_an_exact_callback_boundary_and_drain() {
    use vibez_core::{id::SectionId, midi::MidiNote};
    use vibez_engine::{
        events::EngineEvent,
        playback_source::{EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource},
    };
    use vibez_plugin_host::PluginInstrumentWrapper;
    let fixture = support::Fixture::new();
    for source_format in ["clap", "vst3"] {
        for effect_format in ["clap", "vst3"] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let track = TrackId::new();
            let effect = EffectId::new();
            let section = SectionId::new();
            let mut instrument = fixture.load_instrument(source_format, 64);
            let state: Vec<_> = [137u32, 137, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            instrument.load_state(&state);
            instrument.reconfigure_on_main_thread().unwrap();
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            commands
                .push(EngineCommand::AddMidiTrack(track, "One shot".into()))
                .unwrap();
            commands
                .push(EngineCommand::SetPluginInstrument {
                    track_id: track,
                    instrument: Box::new(PluginInstrumentWrapper::new(instrument)),
                })
                .unwrap();
            commands
                .push(EngineCommand::AddPluginEffect {
                    track_id: track,
                    effect_id: effect,
                    effect: Box::new(PluginEffectWrapper::new(delay(
                        &fixture,
                        effect_format,
                        64,
                        521,
                        521,
                    ))),
                    position: None,
                })
                .unwrap();
            let channels = [channel(track, &[effect]), channel(TrackId::MASTER, &[])];
            let reports = [
                (
                    RoutingNode {
                        channel: track,
                        stage: NodeStage::Source,
                    },
                    137,
                ),
                (
                    RoutingNode {
                        channel: track,
                        stage: NodeStage::Effect(effect),
                    },
                    521,
                ),
            ];
            commands
                .push(EngineCommand::SetRouting(
                    PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1).unwrap(),
                ))
                .unwrap();
            let length = 128.0 / 24000.0;
            let notes = EngineNoteClip::new(
                ClipId::new(),
                0.0,
                length,
                vec![MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: length,
                }],
                0.0,
                false,
                0.0,
                0.0,
                Default::default(),
            );
            commands
                .push(EngineCommand::LaunchSection(Box::new(
                    PreparedSectionPlaybackSource::new(
                        section,
                        length,
                        false,
                        vec![(
                            track,
                            PreparedPlaybackSource::new(vec![], vec![notes], vec![]),
                        )],
                    ),
                )))
                .unwrap();
            let mut output = Vec::new();
            let mut stopped = None;
            for block in 0..20 {
                let mut samples = [0.0; 128];
                engine.process_block(AudioProcessBlock::new(&mut samples, 2));
                output.extend(samples);
                while let Ok(event) = events.pop() {
                    if matches!(event, EngineEvent::PlaybackStopped) {
                        stopped = Some((block + 1) * 64);
                    }
                }
            }
            for (frame, values) in output.chunks_exact(2).enumerate() {
                let expected = if (658..786).contains(&frame) {
                    0.75 * std::f32::consts::FRAC_1_SQRT_2
                } else {
                    0.0
                };
                assert!(
                    (values[0] - expected).abs() < 1e-6,
                    "{source_format}->{effect_format} frame{frame}: {values:?}"
                );
            }
            assert!(stopped.unwrap() >= 786);
        }
    }
}
