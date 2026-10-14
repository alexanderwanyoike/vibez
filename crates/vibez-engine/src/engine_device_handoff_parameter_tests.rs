//! Bounded retained parameter intent preserves coalescing, complete replay and device lifetime.

use super::*;
use compensation_state_test_support::Readiness;
use graph_test_support::{
    constant_clip, graph_engine, routing_channel, routing_effect, routing_plan,
};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::EffectId,
    midi::InstrumentKind,
};

const PARAMETER_CAPACITY: usize = 2048;
const REJECTED_INDEX: usize = PARAMETER_CAPACITY / 2;

struct ReplayAudit {
    calls: AtomicUsize,
    indices: [AtomicUsize; PARAMETER_CAPACITY],
    offered: [AtomicU32; PARAMETER_CAPACITY],
}

impl ReplayAudit {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            indices: std::array::from_fn(|_| AtomicUsize::new(usize::MAX)),
            offered: std::array::from_fn(|_| AtomicU32::new(u32::MAX)),
        }
    }
}

struct ReplayProbe {
    requested: Arc<AtomicBool>,
    audit: Arc<ReplayAudit>,
    values: [f32; PARAMETER_CAPACITY + 1],
    stopped: bool,
}

impl ReplayProbe {
    fn new(requested: Arc<AtomicBool>, audit: Arc<ReplayAudit>) -> Self {
        Self {
            requested,
            audit,
            values: [0.0; PARAMETER_CAPACITY + 1],
            stopped: false,
        }
    }
    fn stop_for_transfer(&mut self) {
        assert!(!self.stopped);
        self.stopped = true;
    }
    fn reactivate(&mut self) -> Result<(), String> {
        assert!(self.stopped);
        self.requested.store(false, Ordering::Release);
        self.stopped = false;
        Ok(())
    }
    fn apply(&mut self, index: usize, value: f32) -> bool {
        let call = self.audit.calls.fetch_add(1, Ordering::Relaxed);
        assert!(call < PARAMETER_CAPACITY);
        self.audit.indices[call].store(index, Ordering::Relaxed);
        self.audit.offered[call].store(value.to_bits(), Ordering::Relaxed);
        if index == REJECTED_INDEX {
            return false;
        }
        if let Some(parameter) = self.values.get_mut(index) {
            *parameter = value;
            true
        } else {
            false
        }
    }
}

impl vibez_dsp::effect::AudioEffect for ReplayProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    fn stop_for_reconfiguration(&mut self) {
        self.stop_for_transfer();
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        self.reactivate()
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.apply(index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.values[index]
    }
    fn process(&mut self, _: &mut [f32], _: usize) {
        assert!(!self.stopped);
    }
    fn reset(&mut self) {}
}

impl vibez_instruments::Instrument for ReplayProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    fn stop_for_reconfiguration(&mut self) {
        self.stop_for_transfer();
    }
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        self.reactivate()
    }
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.apply(index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.values[index]
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, _: &mut [f32], _: usize) {
        assert!(!self.stopped);
    }
    fn reset(&mut self) {}
}

fn parameter_edit(
    track_id: TrackId,
    effect_id: EffectId,
    instrument: bool,
    param_index: usize,
    value: f32,
) -> EngineCommand {
    if instrument {
        EngineCommand::SetInstrumentParam {
            track_id,
            param_index,
            value,
        }
    } else {
        EngineCommand::SetEffectParam {
            track_id,
            effect_id,
            param_index,
            value,
        }
    }
}

fn resident_identity(engine: &AudioEngine, instrument: bool) -> usize {
    if instrument {
        reconfiguration::instrument_owner_identity(&**engine.tracks[0].instrument.as_ref().unwrap())
    } else {
        reconfiguration::effect_owner_identity(&*engine.tracks[0].effects[0].effect)
    }
}

fn resident_value(engine: &AudioEngine, instrument: bool, index: usize) -> f32 {
    if instrument {
        engine.tracks[0]
            .instrument
            .as_ref()
            .unwrap()
            .get_param(index)
    } else {
        engine.tracks[0].effects[0].effect.get_param(index)
    }
}

fn transfer_publicly(
    engine: &mut AudioEngine,
    events: &mut Consumer<EngineEvent>,
    order: &mut Vec<&'static str>,
    label: &'static str,
) -> reconfiguration::DeviceReconfiguration {
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [], 2)),
        (0, 0)
    );
    let mut owner = None;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::DeviceReconfiguration(device) => {
                assert!(owner.is_none());
                owner = Some(device);
                order.push(label);
            }
            EngineEvent::TrackMeter(_)
            | EngineEvent::Metering(_)
            | EngineEvent::TrackNoteActivity(_)
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected parameter handoff event: {event:?}"),
        }
    }
    owner.expect("actual public owner transfer")
}

fn offered_value(index: usize) -> f32 {
    match index {
        0 => 0.875,
        REJECTED_INDEX => 0.625,
        index if index == PARAMETER_CAPACITY - 1 => 0.9375,
        _ => (index + 1) as f32 / 4096.0,
    }
}

#[test]
fn retained_parameter_capacity_coalesces_and_replays_every_entry() {
    for instrument in [false, true] {
        let track_id = TrackId::new();
        let effect_id = EffectId::new();
        let mut channel = routing_channel(track_id);
        if !instrument {
            channel.effects.push(routing_effect(effect_id));
        }
        let channels = [channel, routing_channel(TrackId::MASTER)];
        let (mut engine, mut commands, mut events) = graph_engine(&channels);
        let requested = Arc::new(AtomicBool::new(false));
        let audit = Arc::new(ReplayAudit::new());
        let probe = ReplayProbe::new(Arc::clone(&requested), Arc::clone(&audit));
        let mut track = EngineTrack::new(track_id);
        track.pan = 0.5;
        track
            .playback_source
            .clips
            .push(constant_clip(32_768, 0.25));
        let identity = if instrument {
            let owner: Box<dyn vibez_instruments::Instrument> = Box::new(probe);
            let identity = reconfiguration::instrument_owner_identity(&*owner);
            track.instrument = Some(owner);
            identity
        } else {
            let owner: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(probe);
            let identity = reconfiguration::effect_owner_identity(&*owner);
            track.effects.push(EffectSlot {
                id: effect_id,
                effect: owner,
                bypass: false,
            });
            identity
        };
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
                EngineEvent::TrackMeter(_)
                | EngineEvent::Metering(_)
                | EngineEvent::TrackNoteActivity(_)
                | EngineEvent::PlaybackPosition(_) => {}
                event => panic!("unexpected parameter setup event: {event:?}"),
            }
        }
        assert_eq!(started, 1);
        assert_eq!(audit.calls.load(Ordering::Relaxed), 0);
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
        let away = Readiness {
            suspended: true,
            handoff: true,
            muted: true,
            ..running
        };
        let mut readiness = vec![Readiness::observe(&engine)];
        assert_eq!(readiness[0], running);
        let original_plan = std::ptr::from_ref(engine.routing.as_ref()) as usize;
        let mut order = Vec::new();
        requested.store(true, Ordering::Release);
        let mut owner = transfer_publicly(&mut engine, &mut events, &mut order, "handoff");
        let first_id = owner.handoff_id();
        assert_eq!(first_id, 1);
        assert_eq!(owner.owner_identity(), identity);
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[1], away);
        let storage = engine.device_handoff.retained_parameters().as_ptr();
        let capacity = engine.device_handoff.retained_parameter_capacity();
        assert_eq!(capacity, PARAMETER_CAPACITY);
        for start in (0..PARAMETER_CAPACITY).step_by(256) {
            for index in start..start + 256 {
                commands
                    .push(parameter_edit(
                        track_id,
                        effect_id,
                        instrument,
                        index,
                        (index + 1) as f32 / 4096.0,
                    ))
                    .unwrap();
            }
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.drain_commands()),
                (0, 0)
            );
            assert_eq!(
                engine.device_handoff.retained_parameters().len(),
                start + 256
            );
            assert_eq!(
                engine.device_handoff.retained_parameters().as_ptr(),
                storage
            );
            assert_eq!(
                engine.device_handoff.retained_parameter_capacity(),
                capacity
            );
            assert_eq!(audit.calls.load(Ordering::Relaxed), 0);
            assert_eq!(Readiness::observe(&engine), away);
            assert!(events.is_empty());
        }
        for index in [0, REJECTED_INDEX, PARAMETER_CAPACITY - 1] {
            commands
                .push(parameter_edit(
                    track_id,
                    effect_id,
                    instrument,
                    index,
                    offered_value(index),
                ))
                .unwrap();
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        assert_eq!(
            engine.device_handoff.retained_parameters().len(),
            PARAMETER_CAPACITY
        );
        for (expected, &(index, value)) in engine
            .device_handoff
            .retained_parameters()
            .iter()
            .enumerate()
        {
            assert_eq!(index, expected);
            assert_eq!(value.to_bits(), offered_value(index).to_bits());
        }
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[2], away);
        commands
            .push(parameter_edit(
                track_id,
                effect_id,
                instrument,
                PARAMETER_CAPACITY,
                0.5,
            ))
            .unwrap();
        commands
            .push(EngineCommand::SetTrackSolo(track_id, true))
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        let overflow_consumed = engine.cmd_rx.peek().is_err() && engine.tracks[0].solo;
        assert!(overflow_consumed);
        assert_eq!(
            engine.device_handoff.retained_parameters().len(),
            PARAMETER_CAPACITY
        );
        assert_eq!(
            engine.device_handoff.retained_parameters().as_ptr(),
            storage
        );
        assert_eq!(
            engine.device_handoff.retained_parameter_capacity(),
            capacity
        );
        assert_eq!(audit.calls.load(Ordering::Relaxed), 0);
        let EngineEvent::CompensationInvalid {
            track_id: failed_track,
            effect_id: failed_effect,
            reason,
        } = events.pop().unwrap()
        else {
            panic!("overflow cause")
        };
        assert_eq!(failed_track, track_id);
        assert_eq!(
            failed_effect,
            if instrument { None } else { Some(effect_id) }
        );
        assert_eq!(
            reason,
            "Device recovery parameter queue is full; parameter edit was rejected"
        );
        order.push("overflow-cause");
        assert!(events.is_empty());
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[3], away);
        owner.reconfigure_on_main_thread().unwrap();
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
        let mut replay_rejections = 0;
        let mut returned = 0;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::CompensationInvalid {
                    track_id: failed_track,
                    effect_id: failed_effect,
                    reason,
                } => {
                    assert_eq!(failed_track, track_id);
                    assert_eq!(
                        failed_effect,
                        if instrument { None } else { Some(effect_id) }
                    );
                    assert_eq!(
                        reason,
                        "Recovered device rejected a retained parameter edit"
                    );
                    replay_rejections += 1;
                    order.push("replay-cause");
                }
                EngineEvent::RoutingRetired(plan) => {
                    assert_eq!(std::ptr::from_ref(plan.as_ref()) as usize, original_plan);
                    returned += 1;
                    order.push("original-plan-return");
                }
                event => panic!("unexpected replay event: {event:?}"),
            }
        }
        assert_eq!((replay_rejections, returned), (1, 1));
        assert_eq!(audit.calls.load(Ordering::Relaxed), PARAMETER_CAPACITY);
        let indices: Vec<_> = audit
            .indices
            .iter()
            .map(|value| value.load(Ordering::Relaxed))
            .collect();
        let offered_bits: Vec<_> = audit
            .offered
            .iter()
            .map(|value| value.load(Ordering::Relaxed))
            .collect();
        assert_eq!(indices, (0..PARAMETER_CAPACITY).collect::<Vec<_>>());
        for (index, offered) in offered_bits.iter().enumerate() {
            assert_eq!(*offered, offered_value(index).to_bits());
        }
        let applied_bits: Vec<_> = (0..=PARAMETER_CAPACITY)
            .map(|index| resident_value(&engine, instrument, index).to_bits())
            .collect();
        for (index, applied) in applied_bits.iter().enumerate() {
            let expected = if index == REJECTED_INDEX || index == PARAMETER_CAPACITY {
                0.0
            } else {
                offered_value(index)
            };
            assert_eq!(*applied, expected.to_bits());
        }
        let first_owner_roundtrip = resident_identity(&engine, instrument) == identity;
        assert!(first_owner_roundtrip);
        assert!(engine.device_handoff.retained_parameters().is_empty());
        assert_eq!(
            engine.device_handoff.retained_parameters().as_ptr(),
            storage
        );
        assert_eq!(
            engine.device_handoff.retained_parameter_capacity(),
            capacity
        );
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[4], running);
        requested.store(true, Ordering::Release);
        let mut invalidated_owner =
            transfer_publicly(&mut engine, &mut events, &mut order, "second-handoff");
        let second_id = invalidated_owner.handoff_id();
        assert_eq!(second_id, 2);
        assert_eq!(invalidated_owner.owner_identity(), identity);
        commands
            .push(parameter_edit(track_id, effect_id, instrument, 0, 0.125))
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        assert_eq!(engine.device_handoff.retained_parameters(), [(0, 0.125)]);
        assert_eq!(audit.calls.load(Ordering::Relaxed), PARAMETER_CAPACITY);
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[5], away);
        let replacement_audit = Arc::new(ReplayAudit::new());
        let replacement = ReplayProbe::new(
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&replacement_audit),
        );
        let replacement_identity = if instrument {
            let replacement: Box<dyn vibez_instruments::Instrument> = Box::new(replacement);
            let identity = reconfiguration::instrument_owner_identity(&*replacement);
            commands
                .push(EngineCommand::SetInstrument {
                    track_id,
                    instrument: replacement,
                })
                .unwrap();
            identity
        } else {
            commands
                .push(EngineCommand::RemoveEffect(track_id, effect_id))
                .unwrap();
            let replacement: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(replacement);
            let identity = reconfiguration::effect_owner_identity(&*replacement);
            commands
                .push(EngineCommand::InsertEffect {
                    track_id,
                    effect_id,
                    effect: replacement,
                    position: Some(0),
                })
                .unwrap();
            identity
        };
        assert_ne!(replacement_identity, identity);
        let replacement_plan = routing_plan(&channels);
        let replacement_plan_identity = std::ptr::from_ref(replacement_plan.as_ref()) as usize;
        commands
            .push(EngineCommand::SetRouting(replacement_plan))
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        let mut displaced = 0;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::RoutingRetired(plan) => {
                    assert_eq!(
                        std::ptr::from_ref(plan.as_ref()) as usize,
                        accepted_identity
                    );
                    displaced += 1;
                    order.push("accepted-plan-return");
                }
                event => panic!("unexpected invalidation event: {event:?}"),
            }
        }
        assert_eq!(displaced, 1);
        assert!(engine.device_handoff.retained_parameters().is_empty());
        assert_eq!(
            engine.device_handoff.retained_parameters().as_ptr(),
            storage
        );
        assert_eq!(
            engine.device_handoff.retained_parameter_capacity(),
            capacity
        );
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[6], running);
        invalidated_owner.reconfigure_on_main_thread().unwrap();
        let stale_plan = routing_plan(&channels);
        let stale_plan_identity = std::ptr::from_ref(stale_plan.as_ref()) as usize;
        commands
            .push(EngineCommand::ResumeDeviceReconfiguration {
                device: invalidated_owner,
                routing: stale_plan,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
        let mut retired = 0;
        let mut stale_returns = 0;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::RoutingRetired(plan) => {
                    assert_eq!(
                        std::ptr::from_ref(plan.as_ref()) as usize,
                        stale_plan_identity
                    );
                    stale_returns += 1;
                    order.push("stale-plan-return");
                }
                EngineEvent::DeviceReconfigurationRetired { device, reason } => {
                    assert_eq!(device.owner_identity(), identity);
                    assert_eq!(device.handoff_id(), second_id);
                    assert!(reason.is_none());
                    let original_value = match &device {
                        reconfiguration::DeviceReconfiguration::Effect { slot, .. } => {
                            slot.effect.get_param(0)
                        }
                        reconfiguration::DeviceReconfiguration::Instrument {
                            instrument, ..
                        } => instrument.get_param(0),
                    };
                    assert_eq!(original_value.to_bits(), offered_value(0).to_bits());
                    retired += 1;
                    order.push("old-owner-retired");
                }
                event => panic!("unexpected invalidated reply event: {event:?}"),
            }
        }
        assert_eq!((retired, stale_returns), (1, 1));
        assert_eq!(audit.calls.load(Ordering::Relaxed), PARAMETER_CAPACITY);
        assert_eq!(replacement_audit.calls.load(Ordering::Relaxed), 0);
        let replacement_bits: Vec<_> = (0..=PARAMETER_CAPACITY)
            .map(|index| resident_value(&engine, instrument, index).to_bits())
            .collect();
        assert_eq!(replacement_bits, vec![0; PARAMETER_CAPACITY + 1]);
        let replacement_intact = resident_identity(&engine, instrument) == replacement_identity;
        assert!(replacement_intact);
        assert_eq!(
            std::ptr::from_ref(engine.routing.as_ref()) as usize,
            replacement_plan_identity
        );
        let backing_survived = engine.device_handoff.retained_parameters().as_ptr() == storage
            && engine.device_handoff.retained_parameter_capacity() == capacity;
        assert!(backing_survived);
        assert!(engine.device_handoff.retained_parameters().is_empty());
        readiness.push(Readiness::observe(&engine));
        assert_eq!(readiness[7], running);
        assert_eq!(
            order,
            [
                "handoff",
                "overflow-cause",
                "replay-cause",
                "original-plan-return",
                "second-handoff",
                "accepted-plan-return",
                "stale-plan-return",
                "old-owner-retired"
            ]
        );
    }
}
