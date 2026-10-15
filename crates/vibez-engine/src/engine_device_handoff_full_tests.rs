//! Full handoff publication restores the same stopped owner before a later transfer.

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
use vibez_dsp::factory::create_effect;

#[derive(Default)]
struct RetryCounts {
    stops: AtomicUsize,
    reactivations: AtomicUsize,
    process: AtomicUsize,
}

struct RetryProbe {
    requested: Arc<AtomicBool>,
    counts: Arc<RetryCounts>,
    stopped: bool,
}

impl vibez_dsp::effect::AudioEffect for RetryProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    fn stop_for_reconfiguration(&mut self) {
        // Full restores the stopped Box without reactivating it; retry stops it again.
        self.stopped = true;
        self.counts.stops.fetch_add(1, Ordering::Relaxed);
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        assert!(self.stopped);
        self.counts.reactivations.fetch_add(1, Ordering::Relaxed);
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
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {
        assert!(!self.stopped);
        self.counts.process.fetch_add(1, Ordering::Relaxed);
    }
    fn reset(&mut self) {}
}

#[test]
fn full_publication_restores_same_box_and_consumes_identity_before_retry() {
    for bypass in [false, true] {
        let track_id = TrackId::new();
        let effects = [EffectId::new(), EffectId::new(), EffectId::new()];
        let mut channel = routing_channel(track_id);
        channel.effects = effects.map(routing_effect).into();
        let channels = [channel, routing_channel(TrackId::MASTER)];
        let (mut engine, mut commands, mut events) = graph_engine(&channels);
        let requested = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(RetryCounts::default());
        let mut track = EngineTrack::new(track_id);
        track.pan = 0.5;
        track
            .playback_source
            .clips
            .push(constant_clip(32_768, 0.25));
        let held: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(RetryProbe {
            requested: Arc::clone(&requested),
            counts: Arc::clone(&counts),
            stopped: false,
        });
        let identity = reconfiguration::effect_owner_identity(&*held);
        track.effects = vec![
            EffectSlot {
                id: effects[0],
                effect: create_effect(EffectType::Gain, DEFAULT_SAMPLE_RATE as f32),
                bypass: false,
            },
            EffectSlot {
                id: effects[1],
                effect: held,
                bypass,
            },
            EffectSlot {
                id: effects[2],
                effect: create_effect(EffectType::Gain, DEFAULT_SAMPLE_RATE as f32),
                bypass: false,
            },
        ];
        commands
            .push(EngineCommand::AddTrack(Box::new(track)))
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
                EngineEvent::TrackMeter { .. }
                | EngineEvent::Metering { .. }
                | EngineEvent::PlaybackPosition(_) => {}
                event => panic!("unexpected Full setup event: {event:?}"),
            }
        }
        assert_eq!(started, 1);
        assert!(engine.scheduled_presentation.is_empty());
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
        assert!(level.into_iter().all(|value| value > 0.1));
        let original_plan = std::ptr::from_ref(engine.routing.as_ref()) as usize;
        let storage = engine.tracks[0].effects.as_ptr();
        let capacity = engine.tracks[0].effects.capacity();
        let parameter_storage = engine.device_handoff.retained_parameters().as_ptr();
        let parameter_capacity = engine.device_handoff.retained_parameter_capacity();
        assert_eq!(parameter_capacity, 2048);
        let processed_before = counts.process.load(Ordering::Relaxed);
        let mut filled = 0;
        while engine
            .event_tx
            .push(EngineEvent::PlaybackPosition(u64::MAX))
            .is_ok()
        {
            filled += 1;
        }
        assert_eq!(filled, vibez_core::constants::RING_BUFFER_CAPACITY);
        assert_eq!(engine.event_tx.slots(), 0);
        requested.store(true, Ordering::Release);
        let mut loss = [0.0; 128];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut loss, 2)),
            (0, 0)
        );
        readiness.push(Readiness::observe(&engine));
        assert_eq!(
            readiness[1],
            Readiness {
                restart_requested: true,
                muted: true,
                ..running
            }
        );
        let rejected_identity = engine.device_handoff.last_issued_identity();
        assert_eq!(rejected_identity, 1);
        assert_eq!(counts.stops.load(Ordering::Relaxed), 1);
        assert_eq!(counts.reactivations.load(Ordering::Relaxed), 0);
        assert_eq!(counts.process.load(Ordering::Relaxed), processed_before);
        assert_eq!(engine.tracks[0].effects.as_ptr(), storage);
        assert_eq!(engine.tracks[0].effects.capacity(), capacity);
        assert_eq!(
            engine.tracks[0]
                .effects
                .iter()
                .map(|slot| slot.id)
                .collect::<Vec<_>>(),
            effects
        );
        assert_eq!(engine.tracks[0].effects[1].bypass, bypass);
        assert_eq!(
            reconfiguration::effect_owner_identity(&*engine.tracks[0].effects[1].effect),
            identity
        );
        assert_eq!(
            std::ptr::from_ref(engine.routing.as_ref()) as usize,
            original_plan
        );
        assert_eq!(
            engine.device_handoff.retained_parameters().as_ptr(),
            parameter_storage
        );
        assert_eq!(
            engine.device_handoff.retained_parameter_capacity(),
            parameter_capacity
        );
        assert!(engine.device_handoff.retained_parameters().is_empty());
        assert!(engine.pending_retirements.is_empty());
        assert!(engine.scheduled_presentation.is_empty());
        assert_eq!(engine.event_tx.slots(), 0);
        for (frame, pair) in loss.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * (64 - frame) as f32 / 64.0;
                assert!((pair[channel] - expected).abs() < 1e-7);
            }
        }
        let mut released = 0;
        while let Ok(event) = events.pop() {
            assert!(matches!(event, EngineEvent::PlaybackPosition(u64::MAX)));
            released += 1;
        }
        assert_eq!(released, filled);
        let available_before_retry = engine.event_tx.slots();
        assert_eq!(available_before_retry, released);
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
            (0, 0)
        );
        readiness.push(Readiness::observe(&engine));
        assert_eq!(
            readiness[2],
            Readiness {
                suspended: true,
                handoff: true,
                muted: true,
                ..running
            }
        );
        assert_eq!(engine.device_handoff.last_issued_identity(), 2);
        assert_eq!(counts.stops.load(Ordering::Relaxed), 2);
        assert_eq!(counts.reactivations.load(Ordering::Relaxed), 0);
        assert_eq!(counts.process.load(Ordering::Relaxed), processed_before);
        let pending = engine.device_handoff.pending().unwrap();
        assert_eq!(
            (
                pending.handoff_id,
                pending.track_id,
                pending.effect_id,
                pending.position,
                pending.bypass,
                pending.is_local_recovery()
            ),
            (2, track_id, Some(effects[1]), 1, bypass, false)
        );
        assert_eq!(
            engine.tracks[0]
                .effects
                .iter()
                .map(|slot| slot.id)
                .collect::<Vec<_>>(),
            [effects[0], effects[2]]
        );
        let mut owner = None;
        let mut order = Vec::new();
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::DeviceReconfiguration(device) => {
                    assert!(owner.is_none());
                    assert_eq!(device.owner_identity(), identity);
                    assert_eq!(device.handoff_id(), 2);
                    owner = Some(device);
                    order.push("handoff");
                }
                EngineEvent::PlaybackPosition(_) | EngineEvent::Metering { .. } => {}
                event => panic!("unexpected retry event: {event:?}"),
            }
        }
        let mut owner = owner.expect("one successful transfer after actual ring credit release");
        owner.reconfigure_on_main_thread().unwrap();
        assert_eq!(counts.reactivations.load(Ordering::Relaxed), 1);
        let accepted = routing_plan(&channels);
        let accepted_identity = std::ptr::from_ref(accepted.as_ref()) as usize;
        commands
            .push(EngineCommand::ResumeDeviceReconfiguration {
                device: owner,
                routing: accepted,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        let mut returned = 0;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::RoutingRetired(plan) => {
                    assert_eq!(std::ptr::from_ref(plan.as_ref()) as usize, original_plan);
                    returned += 1;
                    order.push("routing-return");
                }
                event => panic!("unexpected Full resume event: {event:?}"),
            }
        }
        assert_eq!(returned, 1);
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[3], running);
        assert_eq!(
            std::ptr::from_ref(engine.routing.as_ref()) as usize,
            accepted_identity
        );
        assert_eq!(engine.tracks[0].effects.as_ptr(), storage);
        assert_eq!(engine.tracks[0].effects.capacity(), capacity);
        assert_eq!(
            engine.tracks[0]
                .effects
                .iter()
                .map(|slot| slot.id)
                .collect::<Vec<_>>(),
            effects
        );
        assert_eq!(engine.tracks[0].effects[1].bypass, bypass);
        let owner_roundtrip =
            reconfiguration::effect_owner_identity(&*engine.tracks[0].effects[1].effect)
                == identity;
        assert!(owner_roundtrip);
        assert_eq!(
            engine.device_handoff.retained_parameters().as_ptr(),
            parameter_storage
        );
        assert_eq!(
            engine.device_handoff.retained_parameter_capacity(),
            parameter_capacity
        );
        assert!(engine.device_handoff.retained_parameters().is_empty());
        let mut resumed = [0.0; 256];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut resumed, 2)),
            (0, 0)
        );
        for (frame, pair) in resumed.as_chunks::<2>().0.iter().enumerate() {
            for channel in 0..2 {
                let expected = level[channel] * frame.min(64) as f32 / 64.0;
                assert!((pair[channel] - expected).abs() < 1e-7);
            }
        }
        while let Ok(event) = events.pop() {
            assert!(matches!(
                event,
                EngineEvent::TrackMeter { .. }
                    | EngineEvent::Metering { .. }
                    | EngineEvent::PlaybackPosition(_)
            ));
        }
        assert_eq!(Readiness::observe(&engine), running);
        assert_eq!(engine.device_handoff.last_issued_identity(), 2);
        assert_eq!(counts.stops.load(Ordering::Relaxed), 2);
        assert_eq!(counts.reactivations.load(Ordering::Relaxed), 1);
        assert_eq!(engine.output_gate.remaining_resume_frames(), 0);
        assert_eq!(order, ["handoff", "routing-return"]);
    }
}
