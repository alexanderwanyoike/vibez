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
        let (mut engine, mut commands, mut events) = AudioEngine::new();
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
        let meters: Vec<_> = std::iter::from_fn(|| events.pop().ok())
            .filter_map(|event| match event {
                EngineEvent::SidechainInputMeter { peak_l, .. } => Some(peak_l),
                _ => None,
            })
            .collect();
        assert_eq!(
            meters.last().copied(),
            Some(if tap == SourceTap::AfterFader {
                0.0
            } else {
                1.0
            })
        );
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
