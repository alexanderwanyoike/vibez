//! Stale replies retire the old owner without consuming a newer token's parameter intent.

use super::*;
use compensation_state_test_support::Readiness;
use graph_test_support::{
    constant_clip, graph_engine, routing_channel, routing_effect, routing_plan,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
};

struct ParameterOwner {
    requested: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
    values: [f32; 2],
    stopped: bool,
}

impl Drop for ParameterOwner {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

impl vibez_dsp::effect::AudioEffect for ParameterOwner {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    fn stop_for_reconfiguration(&mut self) {
        assert!(!self.stopped);
        self.stopped = true;
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        assert!(self.stopped);
        self.requested.store(false, Ordering::Release);
        self.stopped = false;
        Ok(())
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
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
    fn process(&mut self, _: &mut [f32], _: usize) {
        assert!(!self.stopped);
    }
    fn reset(&mut self) {}
}

fn parameter_command(
    track_id: TrackId,
    effect_id: EffectId,
    param_index: usize,
    value: f32,
) -> EngineCommand {
    EngineCommand::SetEffectParam {
        track_id,
        effect_id,
        param_index,
        value,
    }
}

#[test]
fn stale_old_reply_preserves_new_token_metadata_parameters_and_applied_graph() {
    for rejected in [false, true] {
        for backpressured in [false, true] {
            let tracks = [TrackId::new(), TrackId::new()];
            let effect_id = EffectId::new();
            let mut affected = routing_channel(tracks[0]);
            affected.effects.push(routing_effect(effect_id));
            let channels = [
                affected,
                routing_channel(tracks[1]),
                routing_channel(TrackId::MASTER),
            ];
            let (mut engine, mut commands, mut events) = graph_engine(&channels);
            let old_requested = Arc::new(AtomicBool::new(false));
            let old_drops = Arc::new(AtomicUsize::new(0));
            let new_drops = Arc::new(AtomicUsize::new(0));
            let mut affected = EngineTrack::new(tracks[0]);
            affected.pan = 0.5;
            affected
                .playback_source
                .clips
                .push(constant_clip(32_768, 0.25));
            let old: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(ParameterOwner {
                requested: Arc::clone(&old_requested),
                drops: Arc::clone(&old_drops),
                values: [0.125, 0.25],
                stopped: false,
            });
            let old_identity = reconfiguration::effect_owner_identity(&*old);
            affected.effects.push(EffectSlot {
                id: effect_id,
                effect: old,
                bypass: false,
                levels: Default::default(),
            });
            let mut healthy = EngineTrack::new(tracks[1]);
            healthy.pan = 0.5;
            healthy
                .playback_source
                .clips
                .push(constant_clip(32_768, 0.25));
            commands
                .push(EngineCommand::AddTrack(Box::new(affected)))
                .unwrap();
            commands
                .push(EngineCommand::AddTrack(Box::new(healthy)))
                .unwrap();
            commands.push(EngineCommand::Play).unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 128], 2)),
                (0, 0)
            );
            let mut started = 0;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::PlaybackStarted => started += 1,
                    EngineEvent::TrackMeter(_)
                    | EngineEvent::Metering(_)
                    | EngineEvent::PlaybackPosition(_) => {}
                    event => panic!("unexpected stale setup event: {event:?}"),
                }
            }
            assert_eq!(started, 1);
            let running = Readiness {
                valid: true,
                awaiting_plan: false,
                suspended: false,
                handoff: false,
                restart_requested: false,
                presentation_fault: false,
                playing: true,
                muted: false,
            };
            let mut readiness = vec![Readiness::observe(&engine)];
            assert_eq!(readiness[0], running);
            let level = engine.output_gate.last_frame();
            assert!(level.into_iter().all(|sample| sample > 0.1));
            let original_plan = std::ptr::from_ref(engine.routing.as_ref()) as usize;
            old_requested.store(true, Ordering::Release);
            let mut loss = [0.0; 128];
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut loss, 2)),
                (0, 0)
            );
            let mut old_owner = None;
            let mut order = Vec::new();
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::DeviceReconfiguration(device) => {
                        assert!(old_owner.is_none());
                        assert_eq!(device.owner_identity(), old_identity);
                        assert_eq!(device.handoff_id(), 1);
                        old_owner = Some(device);
                        order.push("old-handoff");
                    }
                    EngineEvent::Metering(_) | EngineEvent::PlaybackPosition(_) => {}
                    event => panic!("unexpected old handoff event: {event:?}"),
                }
            }
            let mut old_owner = old_owner.expect("actual old owner transfer");
            let old_id = old_owner.handoff_id();
            let away = Readiness {
                suspended: true,
                handoff: true,
                muted: true,
                ..running
            };
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[1], away);
            for (frame, pair) in loss.as_chunks::<2>().0.iter().enumerate() {
                for channel in 0..2 {
                    assert!(
                        (pair[channel] - level[channel] * (64 - frame) as f32 / 64.0).abs() < 1e-7
                    );
                }
            }
            for (index, value) in [(0, 0.9), (1, 0.8)] {
                commands
                    .push(parameter_command(tracks[0], effect_id, index, value))
                    .unwrap();
            }
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            assert_eq!(
                engine.device_handoff.retained_parameters(),
                [(0, 0.9), (1, 0.8)]
            );
            commands
                .push(EngineCommand::RemoveEffect(tracks[0], effect_id))
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            assert!(engine.device_handoff.pending().is_none());
            assert!(engine.device_handoff.retained_parameters().is_empty());
            assert!(!engine.device_handoff.is_suspended());
            assert!(engine.compensation_plan.awaiting_structural_plan());
            let new_requested = Arc::new(AtomicBool::new(true));
            let new: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(ParameterOwner {
                requested: Arc::clone(&new_requested),
                drops: Arc::clone(&new_drops),
                values: [0.0; 2],
                stopped: false,
            });
            let new_identity = reconfiguration::effect_owner_identity(&*new);
            assert_ne!(old_identity, new_identity);
            commands
                .push(EngineCommand::InsertEffect {
                    track_id: tracks[0],
                    effect_id,
                    effect: new,
                    position: Some(0),
                })
                .unwrap();
            let current_plan = routing_plan(&channels);
            let current_plan_identity = std::ptr::from_ref(current_plan.as_ref()) as usize;
            commands
                .push(EngineCommand::SetRouting(current_plan))
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            let mut original_returned = 0;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::RoutingRetired(plan) => {
                        assert_eq!(std::ptr::from_ref(plan.as_ref()) as usize, original_plan);
                        original_returned += 1;
                        order.push("original-plan-return");
                    }
                    event => panic!("unexpected replacement publication event: {event:?}"),
                }
            }
            assert_eq!(original_returned, 1);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(
                readiness[2],
                Readiness {
                    restart_requested: true,
                    muted: true,
                    ..running
                }
            );
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
                (0, 0)
            );
            let mut new_owner = None;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::DeviceReconfiguration(device) => {
                        assert!(new_owner.is_none());
                        assert_eq!(device.owner_identity(), new_identity);
                        assert_eq!(device.handoff_id(), 2);
                        new_owner = Some(device);
                        order.push("new-handoff");
                    }
                    EngineEvent::Metering(_) | EngineEvent::PlaybackPosition(_) => {}
                    event => panic!("unexpected new handoff event: {event:?}"),
                }
            }
            let mut new_owner = new_owner.expect("actual newer owner transfer");
            let new_id = new_owner.handoff_id();
            for (index, value) in [(0, 0.5), (1, 0.375), (0, 0.75)] {
                commands
                    .push(parameter_command(tracks[0], effect_id, index, value))
                    .unwrap();
            }
            commands
                .push(EngineCommand::SetEffectBypass {
                    track_id: tracks[0],
                    effect_id,
                    bypass: true,
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            let current = engine.device_handoff.pending().unwrap();
            assert_eq!(
                (
                    current.handoff_id,
                    current.track_id,
                    current.effect_id,
                    current.position,
                    current.bypass,
                    current.is_local_recovery()
                ),
                (new_id, tracks[0], Some(effect_id), 0, true, false)
            );
            assert_eq!(
                engine.device_handoff.retained_parameters(),
                [(0, 0.75), (1, 0.375)]
            );
            let parameter_storage = engine.device_handoff.retained_parameters().as_ptr();
            let parameter_capacity = engine.device_handoff.retained_parameter_capacity();
            let parameters_before: Vec<_> = engine
                .device_handoff
                .retained_parameters()
                .iter()
                .map(|(index, value)| (*index, value.to_bits()))
                .collect();
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[3], away);
            let mut filled = 0;
            if backpressured {
                while engine
                    .event_tx
                    .push(EngineEvent::PlaybackPosition(u64::MAX))
                    .is_ok()
                {
                    filled += 1;
                }
                assert_eq!(filled, vibez_core::constants::RING_BUFFER_CAPACITY);
            }
            let stale_plan = routing_plan(&channels);
            let stale_plan_identity = std::ptr::from_ref(stale_plan.as_ref()) as usize;
            let reason = String::from("stale old owner must not consume newer parameter intent");
            let reason_identity = reason.as_ptr() as usize;
            let command = if rejected {
                drop(stale_plan);
                EngineCommand::RejectDeviceReconfiguration {
                    device: old_owner,
                    reason,
                }
            } else {
                drop(reason);
                old_owner.reconfigure_on_main_thread().unwrap();
                EngineCommand::ResumeDeviceReconfiguration {
                    device: old_owner,
                    routing: stale_plan,
                }
            };
            commands.push(command).unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[4], away);
            let after_stale = engine.device_handoff.pending().unwrap();
            assert_eq!(
                (
                    after_stale.handoff_id,
                    after_stale.track_id,
                    after_stale.effect_id,
                    after_stale.position,
                    after_stale.bypass,
                    after_stale.is_local_recovery()
                ),
                (
                    current.handoff_id,
                    current.track_id,
                    current.effect_id,
                    current.position,
                    current.bypass,
                    current.is_local_recovery()
                )
            );
            assert_eq!(
                engine
                    .device_handoff
                    .retained_parameters()
                    .iter()
                    .map(|(index, value)| (*index, value.to_bits()))
                    .collect::<Vec<_>>(),
                parameters_before
            );
            assert_eq!(
                engine.device_handoff.retained_parameters().as_ptr(),
                parameter_storage
            );
            assert_eq!(
                engine.device_handoff.retained_parameter_capacity(),
                parameter_capacity
            );
            assert_eq!(
                std::ptr::from_ref(engine.routing.as_ref()) as usize,
                current_plan_identity
            );
            assert_eq!(old_drops.load(Ordering::Relaxed), 0);
            assert_eq!(new_drops.load(Ordering::Relaxed), 0);
            let mut released = 0;
            if backpressured {
                assert!(engine.pending_retirements.as_slice().iter().any(|event| matches!(event, EngineEvent::DeviceReconfigurationRetired { device, .. } if device.owner_identity()==old_identity)));
                while let Ok(event) = events.pop() {
                    assert!(matches!(event, EngineEvent::PlaybackPosition(u64::MAX)));
                    released += 1;
                }
                assert_eq!(released, filled);
                assert_eq!(
                    crate::retirement::tests::allocations(|| engine.drain_commands()),
                    (0, 0)
                );
            }
            let mut retired_old = None;
            let mut stale_plan_returns = 0;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::RoutingRetired(plan) => {
                        assert!(!rejected);
                        assert_eq!(
                            std::ptr::from_ref(plan.as_ref()) as usize,
                            stale_plan_identity
                        );
                        stale_plan_returns += 1;
                        order.push("stale-plan-return");
                    }
                    EngineEvent::DeviceReconfigurationRetired { device, reason } => {
                        assert!(retired_old.is_none());
                        assert_eq!(device.owner_identity(), old_identity);
                        assert_eq!(device.handoff_id(), old_id);
                        let reason_roundtrip = if rejected {
                            reason.as_ref().is_some_and(|reason| {
                                reason == "stale old owner must not consume newer parameter intent"
                                    && reason.as_ptr() as usize == reason_identity
                            })
                        } else {
                            reason.is_none()
                        };
                        assert!(reason_roundtrip);
                        let reconfiguration::DeviceReconfiguration::Effect { slot, .. } = &device
                        else {
                            panic!("old effect owner")
                        };
                        let old_parameter_bits = [
                            slot.effect.get_param(0).to_bits(),
                            slot.effect.get_param(1).to_bits(),
                        ];
                        assert_eq!(
                            old_parameter_bits,
                            [0.125_f32.to_bits(), 0.25_f32.to_bits()]
                        );
                        retired_old = Some(device);
                        order.push("stale-owner-retired");
                    }
                    event => panic!("unexpected stale reply event: {event:?}"),
                }
            }
            assert_eq!(stale_plan_returns, usize::from(!rejected));
            assert!(retired_old.is_some());
            assert_eq!(old_drops.load(Ordering::Relaxed), 0);
            drop(retired_old);
            assert_eq!(old_drops.load(Ordering::Relaxed), 1);
            assert_eq!(new_drops.load(Ordering::Relaxed), 0);
            let after_delivery = engine.device_handoff.pending().unwrap();
            assert_eq!(
                (
                    after_delivery.handoff_id,
                    after_delivery.track_id,
                    after_delivery.effect_id,
                    after_delivery.position,
                    after_delivery.bypass,
                    after_delivery.is_local_recovery()
                ),
                (
                    current.handoff_id,
                    current.track_id,
                    current.effect_id,
                    current.position,
                    current.bypass,
                    current.is_local_recovery()
                )
            );
            assert_eq!(
                engine
                    .device_handoff
                    .retained_parameters()
                    .iter()
                    .map(|(index, value)| (*index, value.to_bits()))
                    .collect::<Vec<_>>(),
                parameters_before
            );
            assert_eq!(
                std::ptr::from_ref(engine.routing.as_ref()) as usize,
                current_plan_identity
            );
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[5], away);
            assert!(engine.retired_routing.is_none());
            assert!(engine.pending_retirements.is_empty());
            assert!(engine.tracks[0].effects.is_empty());
            new_owner.reconfigure_on_main_thread().unwrap();
            let accepted = routing_plan(&channels);
            let accepted_identity = std::ptr::from_ref(accepted.as_ref()) as usize;
            commands
                .push(EngineCommand::ResumeDeviceReconfiguration {
                    device: new_owner,
                    routing: accepted,
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            let mut current_returned = 0;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::RoutingRetired(plan) => {
                        assert_eq!(
                            std::ptr::from_ref(plan.as_ref()) as usize,
                            current_plan_identity
                        );
                        current_returned += 1;
                        order.push("current-plan-return");
                    }
                    event => panic!("unexpected current reply event: {event:?}"),
                }
            }
            assert_eq!(current_returned, 1);
            readiness.push(Readiness::observe(&engine));
            assert_eq!(readiness[6], running);
            assert_eq!(
                std::ptr::from_ref(engine.routing.as_ref()) as usize,
                accepted_identity
            );
            assert_eq!(engine.tracks[0].effects.len(), 1);
            let restored = &engine.tracks[0].effects[0];
            assert_eq!(restored.id, effect_id);
            assert!(restored.bypass);
            let owner_roundtrip =
                reconfiguration::effect_owner_identity(&*restored.effect) == new_identity;
            assert!(owner_roundtrip);
            let applied_bits = [
                restored.effect.get_param(0).to_bits(),
                restored.effect.get_param(1).to_bits(),
            ];
            assert_eq!(applied_bits, [0.75_f32.to_bits(), 0.375_f32.to_bits()]);
            assert!(engine.device_handoff.retained_parameters().is_empty());
            assert_eq!(
                engine.device_handoff.retained_parameters().as_ptr(),
                parameter_storage
            );
            assert_eq!(
                engine.device_handoff.retained_parameter_capacity(),
                parameter_capacity
            );
            let mut resumed = [0.0; 256];
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut resumed, 2)),
                (0, 0)
            );
            for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
                for channel in 0..2 {
                    assert!(
                        (pair[channel] - level[channel] * frame.min(64) as f32 / 64.0).abs() < 1e-7
                    );
                }
            }
            while let Ok(event) = events.pop() {
                assert!(matches!(
                    event,
                    EngineEvent::TrackMeter(_)
                        | EngineEvent::Metering(_)
                        | EngineEvent::PlaybackPosition(_)
                ));
            }
            assert_eq!(Readiness::observe(&engine), running);
            let mut expected_order = vec!["old-handoff", "original-plan-return", "new-handoff"];
            if !rejected {
                expected_order.push("stale-plan-return");
            }
            expected_order.extend(["stale-owner-retired", "current-plan-return"]);
            assert_eq!(order, expected_order);
        }
    }
}
