use super::*;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId};
use vibez_core::routing::*;

fn channel(id: TrackId) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: false,
        effects: vec![],
        sends: vec![],
    }
}

fn clip(id: TrackId, value: f32) -> EngineCommand {
    EngineCommand::AddClip {
        track_id: id,
        clip_id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 4096], vec![value; 4096]],
            sample_rate: 44100,
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

fn plan(
    source: TrackId,
    receiver: TrackId,
    effect: EffectId,
    tap: SourceTap,
) -> Box<crate::routing::PreparedRouting> {
    let mut receiver = channel(receiver);
    receiver.effects.push(RoutingEffect {
        id: effect,
        inputs: vec![ExternalInputDescriptor {
            id: ExternalInputId(0),
            name: "Sidechain".into(),
            channels: 2,
        }],
        assignments: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source,
            source_name: "Ghost".into(),
            tap,
        }],
    });
    crate::routing::PreparedRouting::prepare(
        &[receiver, channel(source), channel(TrackId::MASTER)],
        4096,
    )
    .unwrap()
}

#[test]
fn routed_gate_preserves_muted_source_and_solo_without_trigger_leakage() {
    for tap in [
        SourceTap::BeforeEffects,
        SourceTap::AfterEffects,
        SourceTap::AfterFader,
    ] {
        let (mut engine, mut commands, _events) = AudioEngine::new();
        let source = TrackId::new();
        let receiver = TrackId::new();
        let effect = EffectId::new();
        for id in [receiver, source] {
            commands
                .push(EngineCommand::AddTrack(id, "Track".into()))
                .unwrap();
        }
        commands.push(clip(receiver, 0.01)).unwrap();
        commands.push(clip(source, 1.0)).unwrap();
        commands
            .push(EngineCommand::AddEffect {
                track_id: receiver,
                effect_id: effect,
                effect_type: EffectType::Gate,
                position: None,
            })
            .unwrap();
        commands
            .push(EngineCommand::SetEffectParam {
                track_id: receiver,
                effect_id: effect,
                param_index: 0,
                value: -20.0,
            })
            .unwrap();
        commands
            .push(EngineCommand::SetTrackMute(source, true))
            .unwrap();
        commands
            .push(EngineCommand::SetTrackSolo(receiver, true))
            .unwrap();
        commands
            .push(EngineCommand::SetRouting(plan(
                source, receiver, effect, tap,
            )))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = vec![0.0; 4096];
        engine.process(&mut output, 2);
        let tail = &output[output.len() - 128..];
        if tap == SourceTap::AfterFader {
            assert!(tail.iter().all(|sample| sample.abs() < 0.0001));
        } else {
            assert!(tail.iter().all(|sample| *sample > 0.006 && *sample < 0.008));
        }
    }
}

#[test]
fn live_route_swap_retains_effect_state_and_missing_sources_supply_silence() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let source = TrackId::new();
    let receiver = TrackId::new();
    let effect = EffectId::new();
    for id in [receiver, source] {
        commands
            .push(EngineCommand::AddTrack(id, "Track".into()))
            .unwrap();
    }
    commands.push(clip(receiver, 0.01)).unwrap();
    commands.push(clip(source, 1.0)).unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: receiver,
            effect_id: effect,
            effect_type: EffectType::Compressor,
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackMute(source, true))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(plan(
            source,
            receiver,
            effect,
            SourceTap::AfterEffects,
        )))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 64];
    engine.process(&mut output, 2);
    commands
        .push(EngineCommand::SetRouting(plan(
            TrackId::new(),
            receiver,
            effect,
            SourceTap::AfterEffects,
        )))
        .unwrap();
    engine.process(&mut output, 2);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.tracks()[0].effects[0].id, effect);
    let routing = engine.routing.as_ref().unwrap();
    let input = routing
        .nodes
        .iter()
        .flat_map(|node| &node.inputs)
        .next()
        .unwrap();
    assert!(input.connected);
    assert!(input.samples[..64].iter().all(|sample| *sample == 0.0));
}

#[test]
fn soloed_receiver_keeps_bus_detector_source_inaudible() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let ghost = TrackId::new();
    let bass = TrackId::new();
    let bus = TrackId::new();
    let effect = EffectId::new();
    for track in [bass, ghost] {
        commands
            .push(EngineCommand::AddTrack(track, "Track".into()))
            .unwrap();
    }
    commands
        .push(EngineCommand::AddBus(bus, "Detector bus".into()))
        .unwrap();
    commands.push(clip(bass, 0.01)).unwrap();
    commands.push(clip(ghost, 1.0)).unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: ghost,
            bus_id: bus,
            amount: 1.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackSolo(bass, true))
        .unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: bass,
            effect_id: effect,
            effect_type: EffectType::Gate,
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
    let mut ghost_channel = channel(ghost);
    ghost_channel.sends.push((bus, 1.0));
    let mut bus_channel = channel(bus);
    bus_channel.is_bus = true;
    let mut bass_channel = channel(bass);
    bass_channel.effects.push(RoutingEffect {
        id: effect,
        inputs: vec![ExternalInputDescriptor {
            id: ExternalInputId(0),
            name: "Sidechain".into(),
            channels: 2,
        }],
        assignments: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source: bus,
            source_name: "Detector bus".into(),
            tap: SourceTap::AfterEffects,
        }],
    });
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[
                    bass_channel,
                    ghost_channel,
                    bus_channel,
                    channel(TrackId::MASTER),
                ],
                64,
            )
            .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 4096];
    engine.process(&mut output, 2);
    assert!(output[output.len() - 128..]
        .iter()
        .all(|sample| *sample > 0.006 && *sample < 0.008));
    assert!(engine.tracks()[0].sends.is_empty());
}

#[test]
fn clip_mute_points_follow_nonzero_clip_position_inside_a_render_block() {
    use crate::playback_source::{PreparedClipPlayback, PreparedPlaybackSource};
    use vibez_core::{
        automation::{AutomationLane, AutomationPoint, AutomationTarget},
        perform::MusicalBoundary,
    };
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track = TrackId::new();
    let clip_id = ClipId::new();
    let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
    lane.points.push(AutomationPoint {
        beat: 15.0 / 4.0,
        value: 1.0,
        curve: 0.0,
    });
    let source = PreparedPlaybackSource::new(
        vec![EngineClip {
            id: clip_id,
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![1.0; 256]],
                sample_rate: 8,
            }),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 256,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        }],
        vec![],
        vec![lane],
    );
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(120.0),
        EngineCommand::AddTrack(track, "Clip".into()),
        EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[channel(track), channel(TrackId::MASTER)],
                128,
            )
            .unwrap(),
        ),
    ] {
        commands.push(command).unwrap();
    }
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![Box::new(PreparedClipPlayback {
                track_id: track,
                clip_id: Some(clip_id),
                request_id: 1,
                length_samples: 256,
                looping: false,
                source: Box::new(source),
            })],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    engine.process(&mut [0.0; 10], 1);
    assert_eq!(engine.tracks()[0].active_clip.unwrap().position, 10);
    let mut output = [0.0; 20];
    engine.process(&mut output, 1);
    assert_eq!(output[4], 1.0);
    assert_eq!(output[5], 63.0 / 64.0);
    assert_eq!(output[15], 53.0 / 64.0);
}

#[test]
fn live_selected_output_capture_remains_silent_when_solo_excludes_its_source() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let ghost = TrackId::new();
    let bass = TrackId::new();
    let effect = EffectId::new();
    for id in [bass, ghost] {
        commands
            .push(EngineCommand::AddTrack(id, "Track".into()))
            .unwrap();
    }
    commands.push(clip(bass, 0.01)).unwrap();
    commands.push(clip(ghost, 1.0)).unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: bass,
            effect_id: effect,
            effect_type: EffectType::Gate,
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackSolo(bass, true))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(plan(
            ghost,
            bass,
            effect,
            SourceTap::AfterFader,
        )))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 128];
    let mut capture = [1.0; 128];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_track_output_capture(ghost.raw(), &mut capture),
    );
    assert!(capture.iter().all(|sample| *sample == 0.0));
    let input = engine
        .routing
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .flat_map(|node| &node.inputs)
        .next()
        .unwrap();
    assert!(input.samples[..128].iter().any(|sample| *sample > 0.5));
}
