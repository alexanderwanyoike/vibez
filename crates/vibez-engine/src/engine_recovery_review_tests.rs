//! Recovery edits preserve readiness and preallocated retirement ownership.

use super::*;
use vibez_core::{effect::EffectType, id::EffectId, midi::InstrumentKind, routing::*};

fn routing(track: TrackId, effect: Option<EffectId>) -> Box<crate::routing::PreparedRouting> {
    let channels = [
        RoutingChannel {
            id: track,
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
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    crate::routing::PreparedRouting::prepare(&channels, 32).unwrap()
}

#[test]
fn instrument_replacement_with_unchanged_timing_does_not_wait_for_a_topology_edit() {
    let (mut engine, mut commands, _) = AudioEngine::new();
    let track = TrackId::new();
    commands
        .push(EngineCommand::AddMidiTrack(track, "Keys".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(routing(track, None)))
        .unwrap();
    engine.process(&mut [], 2);
    for command in [
        EngineCommand::SetTrackInstrument(track, InstrumentKind::SubtractiveSynth),
        EngineCommand::RemoveTrackInstrument(track),
    ] {
        commands.push(command).unwrap();
        engine.process(&mut [0.0; 16], 2);
        assert!(
            !engine.graph_edit_pending,
            "instrument identity is not graph topology"
        );
        assert!(engine.compensation_valid);
    }
}

#[test]
fn reject_worst_case_retirement_is_reserved_before_command_ownership_is_popped() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let mut channel = EngineTrack::new(track);
    channel.effects.reserve(4);
    while channel.effects.len() < channel.effects.capacity() {
        channel.effects.push(EffectSlot {
            id: EffectId::new(),
            effect: create_effect(EffectType::Gain, 44100.0),
            bypass: false,
        });
    }
    engine.tracks.push(channel);
    engine.pending_device_reconfiguration = Some(reconfiguration::PendingDeviceReconfiguration {
        local_recovery: false,
        handoff_id: 7,
        track_id: track,
        effect_id: Some(effect),
        position: 0,
        bypass: false,
    });
    let reserved_effects = Vec::with_capacity(1);
    let device = reconfiguration::DeviceReconfiguration::Effect {
        handoff_id: 7,
        reserved_effects,
        track_id: track,
        position: 0,
        slot: EffectSlot {
            id: effect,
            effect: create_effect(EffectType::Gain, 44100.0),
            bypass: false,
        },
    };
    while engine.event_tx.push(EngineEvent::PlaybackStopped).is_ok() {}
    for _ in 0..engine.pending_retirements.capacity() - 2 {
        engine
            .pending_retirements
            .push(EngineEvent::PlaybackStopped);
    }
    let capacity = engine.pending_retirements.capacity();
    commands
        .push(EngineCommand::RejectDeviceReconfiguration {
            device,
            reason: "Rejected native restart".into(),
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.capacity(), capacity);
    while events.pop().is_ok() {}
    engine.drain_commands();
}

struct ParameterProbe {
    requested: bool,
    values: [f32; 2],
}
impl vibez_dsp::effect::AudioEffect for ParameterProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        self.requested = false;
        Ok(())
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if let Some(parameter) = self.values.get_mut(index) {
            *parameter = value;
            true
        } else {
            false
        }
    }
    fn get_param(&self, index: usize) -> f32 {
        self.values[index]
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn parameter_edits_while_main_holds_the_owner_are_coalesced_and_restored_without_allocation() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let mut channel = EngineTrack::new(track);
    channel.effects.push(EffectSlot {
        id: effect,
        bypass: false,
        effect: Box::new(ParameterProbe {
            requested: true,
            values: [0.0; 2],
        }),
    });
    engine.tracks.push(channel);
    engine.routing = Some(routing(track, Some(effect)));
    engine.begin_device_reconfiguration();
    let EngineEvent::DeviceReconfiguration(mut device) = events.pop().unwrap() else {
        panic!("owner");
    };
    for (param_index, value) in [(0, 0.25), (1, 0.5), (0, 0.75)] {
        commands
            .push(EngineCommand::SetEffectParam {
                track_id: track,
                effect_id: effect,
                param_index,
                value,
            })
            .unwrap();
    }
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.pending_device_parameters, [(0, 0.75), (1, 0.5)]);
    device.reconfigure_on_main_thread().unwrap();
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: routing(track, Some(effect)),
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.tracks[0].effects[0].effect.get_param(0), 0.75);
    assert_eq!(engine.tracks[0].effects[0].effect.get_param(1), 0.5);
    assert!(engine.pending_device_parameters.is_empty());
}

#[test]
fn parameter_intents_do_not_cross_a_replaced_device_lifetime() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let mut channel = EngineTrack::new(track);
    channel.effects.push(EffectSlot {
        id: effect,
        bypass: false,
        effect: Box::new(ParameterProbe {
            requested: true,
            values: [0.0; 2],
        }),
    });
    engine.tracks.push(channel);
    engine.begin_device_reconfiguration();
    let EngineEvent::DeviceReconfiguration(device) = events.pop().unwrap() else {
        panic!("owner");
    };
    commands
        .push(EngineCommand::SetEffectParam {
            track_id: track,
            effect_id: effect,
            param_index: 0,
            value: 0.75,
        })
        .unwrap();
    commands
        .push(EngineCommand::RemoveEffect(track, effect))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(ParameterProbe {
                requested: false,
                values: [0.0; 2],
            }),
            position: None,
        })
        .unwrap();
    engine.drain_commands();
    assert!(engine.pending_device_parameters.is_empty());
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: routing(track, Some(effect)),
        })
        .unwrap();
    engine.drain_commands();
    assert_eq!(engine.tracks[0].effects[0].effect.get_param(0), 0.0);
}

impl vibez_instruments::Instrument for ParameterProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        self.requested = false;
        Ok(())
    }
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        vibez_dsp::effect::AudioEffect::set_param(self, index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.values[index]
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn instrument_parameter_edits_survive_the_main_thread_handoff() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let mut channel = EngineTrack::new(track);
    channel.instrument = Some(Box::new(ParameterProbe {
        requested: true,
        values: [0.0; 2],
    }));
    engine.tracks.push(channel);
    engine.begin_device_reconfiguration();
    let EngineEvent::DeviceReconfiguration(mut device) = events.pop().unwrap() else {
        panic!("owner");
    };
    commands
        .push(EngineCommand::SetInstrumentParam {
            track_id: track,
            param_index: 0,
            value: 0.5,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    device.reconfigure_on_main_thread().unwrap();
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: routing(track, None),
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(
        engine.tracks[0].instrument.as_ref().unwrap().get_param(0),
        0.5
    );
}
