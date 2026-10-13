//! Delayed native owner returns must not replace devices from a newer lifetime.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;
use vibez_core::midi::InstrumentKind;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

struct EffectProbe(bool);
impl vibez_dsp::effect::AudioEffect for EffectProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.0
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
        if self.0 {
            1.0
        } else {
            2.0
        }
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

struct InstrumentProbe {
    requested: bool,
    drops: Arc<AtomicUsize>,
}
impl Drop for InstrumentProbe {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
impl vibez_instruments::Instrument for InstrumentProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested
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
        if self.requested {
            1.0
        } else {
            2.0
        }
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

fn plan(
    id: TrackId,
    effects: &[EffectId],
    generation: u64,
) -> Box<crate::routing::PreparedRouting> {
    let channels = [
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: effects
                .iter()
                .map(|&id| RoutingEffect {
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
    crate::routing::PreparedRouting::prepare_compensated(&channels, 32, &[], &[], generation)
        .unwrap()
}

fn handed_off(
    engine: &mut AudioEngine,
    events: &mut Consumer<EngineEvent>,
) -> reconfiguration::DeviceReconfiguration {
    engine.begin_device_reconfiguration();
    while let Ok(event) = events.pop() {
        if let EngineEvent::DeviceReconfiguration(device) = event {
            return device;
        }
    }
    panic!("native owner was not handed to the UI");
}

#[test]
fn delayed_effect_return_cannot_resurrect_owner_into_reopened_same_id_channel() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let id = TrackId::new();
    let effect = EffectId::new();
    let mut track = EngineTrack::new(id);
    track.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(EffectProbe(true)),
        bypass: false,
    });
    engine.tracks.push(track);
    let device = handed_off(&mut engine, &mut events);
    commands.push(EngineCommand::RemoveTrack(id)).unwrap();
    commands
        .push(EngineCommand::AddTrack(id, "Reopened".into()))
        .unwrap();
    let ids = [effect, EffectId::new(), EffectId::new(), EffectId::new()];
    for &effect_id in &ids {
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: id,
                effect_id,
                effect: Box::new(EffectProbe(false)),
                position: None,
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::SetRouting(plan(id, &ids, 22)))
        .unwrap();
    engine.drain_commands();
    while engine.event_tx.push(EngineEvent::PlaybackStopped).is_ok() {}
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: plan(id, &[effect], 41),
        })
        .unwrap();
    let allocation_counts = crate::retirement::tests::allocations(|| engine.drain_commands());
    assert_eq!(
        engine.tracks[0].effects.len(),
        4,
        "old owner was inserted into the reopened channel"
    );
    assert_eq!(engine.tracks[0].effects[0].effect.get_param(0), 2.0);
    assert_eq!(engine.routing.as_ref().unwrap().compensation.generation, 22);
    assert_eq!(
        allocation_counts,
        (0, 0),
        "stale returns must retire outside the callback"
    );
}

#[test]
fn delayed_instrument_return_keeps_replacement_and_does_not_drop_it_in_callback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let id = TrackId::new();
    let old_drops = Arc::new(AtomicUsize::new(0));
    let replacement_drops = Arc::new(AtomicUsize::new(0));
    let mut track = EngineTrack::new(id);
    track.instrument = Some(Box::new(InstrumentProbe {
        requested: true,
        drops: old_drops,
    }));
    engine.tracks.push(track);
    let device = handed_off(&mut engine, &mut events);
    commands
        .push(EngineCommand::SetPluginInstrument {
            track_id: id,
            instrument: Box::new(InstrumentProbe {
                requested: false,
                drops: Arc::clone(&replacement_drops),
            }),
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(plan(id, &[], 22)))
        .unwrap();
    engine.drain_commands();
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: plan(id, &[], 41),
        })
        .unwrap();
    let allocation_counts = crate::retirement::tests::allocations(|| engine.drain_commands());
    assert_eq!(
        replacement_drops.load(Ordering::Relaxed),
        0,
        "replacement native owner dropped in callback"
    );
    assert_eq!(
        engine.tracks[0].instrument.as_ref().unwrap().get_param(0),
        2.0
    );
    assert_eq!(engine.routing.as_ref().unwrap().compensation.generation, 22);
    assert_eq!(allocation_counts, (0, 0));
}

#[test]
fn stopped_effect_retains_bypass_and_reorder_edits_when_owner_returns() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let ids = [EffectId::new(), EffectId::new(), EffectId::new()];
    let mut track = EngineTrack::new(track_id);
    for (index, id) in ids.into_iter().enumerate() {
        track.effects.push(EffectSlot {
            id,
            effect: Box::new(EffectProbe(index == 1)),
            bypass: false,
        });
    }
    engine.tracks.push(track);
    let device = handed_off(&mut engine, &mut events);
    commands
        .push(EngineCommand::SetEffectBypass {
            track_id,
            effect_id: ids[1],
            bypass: true,
        })
        .unwrap();
    commands
        .push(EngineCommand::MoveEffect {
            track_id,
            effect_id: ids[1],
            new_index: 2,
        })
        .unwrap();
    engine.drain_commands();
    commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device,
            routing: plan(track_id, &[ids[0], ids[2], ids[1]], 22),
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    let slots = &engine.tracks[0].effects;
    assert_eq!(
        slots.iter().map(|slot| slot.id).collect::<Vec<_>>(),
        [ids[0], ids[2], ids[1]]
    );
    assert!(slots[2].bypass);
}

#[test]
fn stale_reject_with_full_event_ring_retains_device_reason_and_current_plan() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let id = TrackId::new();
    let effect = EffectId::new();
    let mut track = EngineTrack::new(id);
    track.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(EffectProbe(true)),
        bypass: false,
    });
    engine.tracks.push(track);
    let device = handed_off(&mut engine, &mut events);
    commands
        .push(EngineCommand::RemoveEffect(id, effect))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(plan(id, &[], 22)))
        .unwrap();
    engine.drain_commands();
    while engine.event_tx.push(EngineEvent::PlaybackStopped).is_ok() {}
    commands
        .push(EngineCommand::RejectDeviceReconfiguration {
            device,
            reason: "Stale rejected owner".into(),
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert_eq!(engine.routing.as_ref().unwrap().compensation.generation, 22);
    assert!(engine.compensation_valid);
    assert!(matches!(
        engine.pending_retirements.last(),
        Some(EngineEvent::DeviceReconfigurationRetired {
            reason: Some(_),
            ..
        })
    ));
}

#[test]
fn additions_around_held_slot_use_logical_order_and_main_reserved_capacity() {
    for position in [Some(0), Some(2), Some(3), None] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track_id = TrackId::new();
        let ids = [
            EffectId::new(),
            EffectId::new(),
            EffectId::new(),
            EffectId::new(),
        ];
        let mut track = EngineTrack::new(track_id);
        track.effects = Vec::with_capacity(4);
        for (index, id) in ids.into_iter().enumerate() {
            track.effects.push(EffectSlot {
                id,
                effect: Box::new(EffectProbe(index == 1)),
                bypass: false,
            });
        }
        engine.tracks.push(track);
        let mut device = handed_off(&mut engine, &mut events);
        let added = EffectId::new();
        let mut expected = ids.to_vec();
        expected.insert(position.unwrap_or(expected.len()), added);
        device.prepare_effect_storage(expected.len());
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id,
                effect_id: added,
                effect: Box::new(EffectProbe(false)),
                position,
            })
            .unwrap();
        commands
            .push(EngineCommand::ResumeDeviceReconfiguration {
                device,
                routing: plan(track_id, &expected, 22),
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        assert_eq!(
            engine.tracks[0]
                .effects
                .iter()
                .map(|slot| slot.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(engine.compensation_valid);
        assert!(!engine.compensation_suspended);
        assert!(std::iter::from_fn(|| events.pop().ok())
            .any(|event| matches!(event, EngineEvent::RetiredEffectStorage(_))));
    }
}
