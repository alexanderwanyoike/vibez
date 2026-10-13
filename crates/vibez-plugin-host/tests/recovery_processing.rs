//! Source-time recovery keeps healthy branches audible and owned devices guarded.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use std::sync::Arc;
use vibez_core::{
    audio_buffer::DecodedAudio,
    id::{ClipId, EffectId, TrackId},
    routing::*,
};
use vibez_engine::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
    events::EngineEvent,
    routing::PreparedRouting,
};
use vibez_plugin_host::PluginEffectWrapper;

fn channel(id: TrackId, effect: Option<EffectId>) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: false,
        sends: vec![],
        effects: effect
            .into_iter()
            .map(|id| RoutingEffect {
                id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            })
            .collect(),
    }
}

fn clip(track: TrackId, value: f32) -> EngineCommand {
    EngineCommand::AddClip {
        track_id: track,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 4096]; 2],
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
fn source_time_recovery_keeps_the_healthy_branch_audible_and_guards_the_absent_insert() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        for bypass_while_held in [false, true] {
            let tracks = [TrackId::new(), TrackId::new()];
            let effects = [EffectId::new(), EffectId::new()];
            let channels = [
                channel(tracks[0], Some(effects[0])),
                channel(tracks[1], Some(effects[1])),
                channel(TrackId::MASTER, None),
            ];
            let plan = || PreparedRouting::prepare(&channels, 64).unwrap();
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
            for index in 0..2 {
                let mut plugin = fixture.load(format, 64);
                let delay: u32 = 0;
                let flags: u32 = if index == 0 { 1 << 24 } else { 0 };
                let bytes: Vec<_> = [delay, delay, flags]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect();
                assert!(plugin.load_state(&bytes));
                plugin.reconfigure_on_main_thread().unwrap();
                commands
                    .push(EngineCommand::AddTrack(tracks[index], "Constant".into()))
                    .unwrap();
                commands
                    .push(clip(tracks[index], if index == 0 { 0.4 } else { 0.2 }))
                    .unwrap();
                commands
                    .push(EngineCommand::AddPluginEffect {
                        track_id: tracks[index],
                        effect_id: effects[index],
                        effect: Box::new(PluginEffectWrapper::new(plugin)),
                        position: None,
                    })
                    .unwrap();
            }
            commands.push(EngineCommand::SetRouting(plan())).unwrap();
            commands.push(EngineCommand::Play).unwrap();
            commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
            let mut output = [0.0_f32; 128];
            let mut owner = None;
            let mut named_failure = false;
            let mut capture_stops = 0;
            for _ in 0..12 {
                assert_eq!(
                    support::allocation::count_activity(
                        || engine.process_block(AudioProcessBlock::new(&mut output, 2))
                    ),
                    (0, 0)
                );
                while let Ok(event) = events.pop() {
                    match event {
                        EngineEvent::DeviceReconfiguration(device) => {
                            assert!(owner.is_none());
                            owner = Some(device);
                        }
                        EngineEvent::DeviceProcessingFailed {
                            track_id,
                            effect_id,
                            ..
                        } => {
                            assert_eq!(track_id, tracks[0]);
                            assert_eq!(effect_id, Some(effects[0]));
                            named_failure = true;
                        }
                        EngineEvent::PerformanceCaptureStopped { .. } => capture_stops += 1,
                        EngineEvent::PlaybackStopped => {
                            panic!("local failure stopped unaffected playback")
                        }
                        EngineEvent::CompensationInvalid { reason, .. } => {
                            panic!("local failure invalidated graph: {reason}")
                        }
                        _ => {}
                    }
                }
            }
            assert!(named_failure);
            assert_eq!(capture_stops, 1);
            let healthy = [output[0], output[1]];
            assert!(
                (healthy[0] - 0.2 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
                "absent insert leaked dry audio: {healthy:?}"
            );
            assert!(
                output.chunks_exact(2).all(|frame| frame == healthy),
                "absent insert leaked its dry input"
            );
            if bypass_while_held {
                commands
                    .push(EngineCommand::SetEffectBypass {
                        track_id: tracks[0],
                        effect_id: effects[0],
                        bypass: true,
                    })
                    .unwrap();
                let mut bypassed = Vec::new();
                for _ in 0..10 {
                    assert_eq!(
                        support::allocation::count_activity(
                            || engine.process_block(AudioProcessBlock::new(&mut output, 2))
                        ),
                        (0, 0)
                    );
                    bypassed.extend_from_slice(&output);
                    while events.pop().is_ok() {}
                }
                for (frame, pair) in bypassed.chunks_exact(2).enumerate() {
                    for channel in 0..2 {
                        let expected = healthy[channel] * 3.0;
                        assert!(
                            (pair[channel] - expected).abs() < 1e-6,
                            "{format} held bypass frame {frame}: {} != {expected}",
                            pair[channel]
                        );
                    }
                }
            }
            let mut owner = owner.expect("automatic main-thread handoff");
            assert!(owner.processing_recovery_requested());
            owner.reconfigure_on_main_thread().unwrap();
            commands
                .push(EngineCommand::ResumeDeviceReconfiguration {
                    device: owner,
                    routing: plan(),
                })
                .unwrap();
            let mut recovered = Vec::new();
            for _ in 0..10 {
                assert_eq!(
                    support::allocation::count_activity(
                        || engine.process_block(AudioProcessBlock::new(&mut output, 2))
                    ),
                    (0, 0)
                );
                recovered.extend_from_slice(&output);
                while events.pop().is_ok() {}
            }
            for (frame, pair) in recovered.chunks_exact(2).enumerate() {
                for channel in 0..2 {
                    let expected = healthy[channel] * 3.0;
                    assert!(
                        (pair[channel] - expected).abs() < 1e-6,
                        "{format} frame {frame}: {} != {expected}",
                        pair[channel]
                    );
                }
            }
        }
    }
}

#[test]
fn owner_commands_for_missing_targets_retire_on_main_without_callback_destruction() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let audit = unsafe {
            libloading::Library::new(if format == "clap" {
                &fixture.clap
            } else {
                &fixture.module
            })
            .unwrap()
        };
        let (engine, mut commands, mut events) = AudioEngine::new();
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: TrackId::new(),
                effect_id: EffectId::new(),
                effect: Box::new(PluginEffectWrapper::new(fixture.load(format, 64))),
                position: None,
            })
            .unwrap();
        commands
            .push(EngineCommand::SetPluginInstrument {
                track_id: TrackId::new(),
                instrument: Box::new(vibez_plugin_host::PluginInstrumentWrapper::new(
                    fixture.load_instrument(format, 64),
                )),
            })
            .unwrap();
        let engine = std::thread::spawn(move || {
            let mut engine = engine;
            assert_eq!(
                support::allocation::count_activity(
                    || engine.process_block(AudioProcessBlock::new(&mut [], 2))
                ),
                (0, 0)
            );
            engine
        })
        .join()
        .unwrap();
        let mut retired = 0;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::DisposeEffect(owner) => {
                    drop(owner.take());
                    retired += 1;
                }
                EngineEvent::DisposeInstrument(owner) => {
                    drop(owner.take());
                    retired += 1;
                }
                _ => {}
            }
        }
        assert_eq!(retired, 2);
        drop(engine);
        let violations = unsafe {
            audit
                .get::<unsafe extern "C" fn() -> u32>(b"fixture_lifecycle_errors\0")
                .unwrap()()
        };
        assert_eq!(violations, 0, "{format} lifecycle role violation");
    }
}
