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

#[test]
fn full_ring_retains_the_first_processing_failure_and_prioritizes_it_next_callback() {
    use vibez_core::effect::ParamDescriptor;
    struct Failure {
        pending: Option<&'static str>,
        failed: bool,
    }
    impl vibez_dsp::effect::AudioEffect for Failure {
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
        fn process(&mut self, output: &mut [f32], _: usize) {
            if !output.is_empty() && !self.failed {
                self.pending = Some("Controlled failure");
                self.failed = true;
            }
        }
        fn reset(&mut self) {}
        fn take_processing_error(&mut self) -> Option<&'static str> {
            self.pending.take()
        }
    }
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Failure".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: track,
            effect_id: effect,
            effect_type: EffectType::Gain,
            position: None,
        })
        .unwrap();
    let mut model = channel(track);
    model.effects.push(RoutingEffect {
        id: effect,
        inputs: vec![],
        assignments: vec![],
        inactive_inputs: vec![],
    });
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&[model, channel(TrackId::MASTER)], 32)
                .unwrap(),
        ))
        .unwrap();
    engine.process(&mut [], 2);
    engine.tracks[0].effects[0].effect = Box::new(Failure {
        pending: None,
        failed: false,
    });
    while events.pop().is_ok() {}
    while !engine.event_tx.is_full() {
        engine.process(&mut [], 2);
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 2)),
        (0, 0)
    );
    events.pop().unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 2)),
        (0, 0)
    );
    let mut failures = 0;
    while let Ok(event) = events.pop() {
        if let EngineEvent::DeviceProcessingFailed {
            track_id,
            effect_id,
            reason,
        } = event
        {
            assert_eq!(
                (track_id, effect_id, reason),
                (track, Some(effect), "Controlled failure")
            );
            failures += 1;
        }
    }
    assert_eq!(failures, 1);
    assert!(engine.take_device_processing_error().is_none());
}

#[test]
fn hardware_first_pair_live_capture_and_populated_graph_are_allocation_free() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let source = TrackId::new();
    let receiver = TrackId::new();
    let bus = TrackId::new();
    let effect = EffectId::new();
    for id in [source, receiver] {
        commands
            .push(EngineCommand::AddTrack(id, "Track".into()))
            .unwrap();
        commands.push(clip(id, 0.1)).unwrap();
    }
    commands
        .push(EngineCommand::AddBus(bus, "Bus".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: receiver,
            effect_id: effect,
            effect_type: EffectType::Gate,
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: source,
            bus_id: bus,
            amount: 1.0,
        })
        .unwrap();
    let mut source_model = channel(source);
    source_model.sends.push(bus);
    let mut bus_model = channel(bus);
    bus_model.is_bus = true;
    let mut receiver_model = channel(receiver);
    receiver_model.effects.push(RoutingEffect {
        id: effect,
        inactive_inputs: vec![],
        inputs: vec![ExternalInputDescriptor {
            id: ExternalInputId(0),
            name: "Sidechain".into(),
            channels: 2,
        }],
        assignments: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source: bus,
            source_name: "Bus".into(),
            tap: SourceTap::AfterEffects,
        }],
    });
    let channels = vec![
        source_model,
        receiver_model,
        bus_model,
        channel(TrackId::MASTER),
    ];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 16).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 6 * 32];
    let mut capture = [0.0; 6 * 32];
    engine.process(&mut output, 6);
    while events.pop().is_ok() {}
    let input = [0.2; 6 * 32];
    let count = crate::retirement::tests::allocations(|| {
        engine.process_block(
            AudioProcessBlock::new(&mut output, 6)
                .with_live_input(source.raw(), &input)
                .with_track_output_capture(source.raw(), &mut capture),
        )
    });
    assert_eq!(count, (0, 0));
    for (output, capture) in output.chunks_exact(6).zip(capture.chunks_exact(6)) {
        assert!(output[0] > 0.0 && output[1] > 0.0);
        assert!(capture[0] > 0.1 && capture[1] > 0.1);
        assert_eq!(&output[2..], &[0.0; 4]);
        assert_eq!(&capture[2..], &[0.0; 4]);
    }
    let mut short_capture = [0.0; 4];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process_block(
            AudioProcessBlock::new(&mut output, 6)
                .with_live_input(source.raw(), &[0.2; 4])
                .with_track_output_capture(source.raw(), &mut short_capture)
        )),
        (0, 0)
    );
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 16).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 6)),
        (0, 0)
    );
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::ReorderTracks(vec![receiver, source]))
        .unwrap();
    engine.process(&mut output, 6);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 6)),
        (0, 0)
    );
    assert!(output[0] > 0.0);
}

#[test]
fn clip_bus_and_master_automation_advance_on_perform_clock() {
    use crate::playback_source::ActiveClipPlayback;
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    for target in [TrackId::MASTER, TrackId::new()] {
        let (mut engine, mut commands, _events) = AudioEngine::new();
        let track = TrackId::new();
        commands.push(EngineCommand::SetSampleRate(8)).unwrap();
        commands.push(EngineCommand::SetBpm(120.0)).unwrap();
        commands
            .push(EngineCommand::AddTrack(track, "Clip".into()))
            .unwrap();
        commands.push(clip(track, 1.0)).unwrap();
        let mut track_model = channel(track);
        let mut channels = vec![];
        if !target.is_master() {
            commands
                .push(EngineCommand::AddBus(target, "Return".into()))
                .unwrap();
            commands
                .push(EngineCommand::SetSend {
                    track_id: track,
                    bus_id: target,
                    amount: 1.0,
                })
                .unwrap();
            track_model.sends.push(target);
            let mut bus = channel(target);
            bus.is_bus = true;
            channels.push(bus);
        }
        channels.push(track_model);
        channels.push(channel(TrackId::MASTER));
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(&channels, 64).unwrap(),
            ))
            .unwrap();
        engine.process(&mut [], 2);
        let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
        lane.points = vec![
            AutomationPoint {
                beat: 0.0,
                value: 0.5,
                curve: 0.0,
            },
            AutomationPoint {
                beat: 2.0,
                value: 0.0,
                curve: 0.0,
            },
        ];
        engine
            .channel_mut(target)
            .unwrap()
            .playback_source
            .automation = vec![lane];
        engine.clip_performance = true;
        engine.transport.play();
        engine.tracks[0].launcher_source = std::mem::take(&mut engine.tracks[0].playback_source);
        engine.tracks[0].active_clip = Some(ActiveClipPlayback {
            clip_id: ClipId::new(),
            request_id: 1,
            position: 0,
            length: 4096,
            looping: true,
        });
        let mut first = [0.0; 16];
        engine.process(&mut first, 2);
        let mut next = [0.0; 16];
        engine.process(&mut next, 2);
        assert!(
            first[0] > next[0],
            "{target:?} automation stayed at beat zero"
        );
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
        inactive_inputs: Vec::new(),
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
    ghost_channel.sends.push(bus);
    let mut bus_channel = channel(bus);
    bus_channel.is_bus = true;
    let mut bass_channel = channel(bass);
    bass_channel.effects.push(RoutingEffect {
        inactive_inputs: Vec::new(),
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
    assert!(
        engine
            .tracks()
            .iter()
            .find(|track| track.id == bass)
            .unwrap()
            .solo
    );
    assert!(!engine.buses[0].solo);
}

#[test]
fn master_receives_a_muted_prefader_source() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let ghost = TrackId::new();
    let bass = TrackId::new();
    let effect = EffectId::new();
    for track in [bass, ghost] {
        commands
            .push(EngineCommand::AddTrack(track, "Track".into()))
            .unwrap();
    }
    commands.push(clip(bass, 0.01)).unwrap();
    commands.push(clip(ghost, 1.0)).unwrap();
    commands
        .push(EngineCommand::SetTrackMute(ghost, true))
        .unwrap();
    commands
        .push(EngineCommand::AddEffect {
            track_id: TrackId::MASTER,
            effect_id: effect,
            effect_type: EffectType::Gate,
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id: TrackId::MASTER,
            effect_id: effect,
            param_index: 0,
            value: -20.0,
        })
        .unwrap();

    let mut bass_model = channel(bass);
    bass_model.effects.clear();
    let mut master = channel(TrackId::MASTER);
    master.effects.push(RoutingEffect {
        inactive_inputs: Vec::new(),
        id: effect,
        inputs: vec![ExternalInputDescriptor {
            id: ExternalInputId(0),
            name: "Sidechain".into(),
            channels: 2,
        }],
        assignments: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source: ghost,
            source_name: "Ghost".into(),
            tap: SourceTap::AfterEffects,
        }],
    });
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&[bass_model, channel(ghost), master], 4096)
                .unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 4096];
    engine.process(&mut output, 2);
    assert!(output[output.len() - 128..]
        .iter()
        .all(|sample| *sample > 0.006 && *sample < 0.008));
}

#[test]
fn saturated_meter_ring_retains_and_eventually_returns_old_plans() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine.routing =
        Some(crate::routing::PreparedRouting::prepare(&[channel(TrackId::MASTER)], 8).unwrap());
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&[channel(TrackId::MASTER)], 16).unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&[channel(TrackId::MASTER)], 32).unwrap(),
        ))
        .unwrap();
    engine.process(&mut [], 2);
    assert_eq!(engine.routing.as_ref().unwrap().max_frames, 16);
    assert_eq!(engine.retired_routing.as_ref().unwrap().max_frames, 8);
    while events.pop().is_ok() {}
    engine.process(&mut [], 2);
    assert_eq!(engine.routing.as_ref().unwrap().max_frames, 32);
    assert!(engine.retired_routing.is_none());
    let mut returned = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::RoutingRetired(retired) = event {
            returned.push(retired.max_frames);
        }
    }
    assert_eq!(returned, vec![8, 16]);
}

#[test]
fn prepared_send_edges_follow_automation_without_mutating_manual_sends() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let source = TrackId::new();
    let bus = TrackId::new();
    commands
        .push(EngineCommand::AddTrack(source, "Source".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddBus(bus, "Return".into()))
        .unwrap();
    commands.push(clip(source, 1.0)).unwrap();
    let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id: bus });
    lane.points.push(AutomationPoint {
        beat: 0.0,
        value: 0.5,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: source,
            lane,
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
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 32];
    engine.process(&mut output, 2);
    assert!(output
        .iter()
        .all(|sample| (*sample - 1.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6));
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
