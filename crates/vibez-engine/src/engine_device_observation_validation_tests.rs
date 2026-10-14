//! Ordered validation and post-stop recovery sampling preserve failure and handoff boundaries.

use super::*;
use graph_test_support::{constant_clip, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;
use vibez_core::midi::InstrumentKind;
use vibez_core::routing::RoutingChannel;

const RATE: u64 = 1;
const VALID: u64 = 2;
const LOCAL: u64 = 3;
const LATENCY: u64 = 4;
const REQUEST: u64 = 5;
const RECOVERY: u64 = 6;
const STOP: u64 = 7;

struct Trace {
    calls: [AtomicU64; 256],
    len: AtomicUsize,
    overflow: AtomicBool,
}

impl Trace {
    fn new() -> Self {
        Self {
            calls: std::array::from_fn(|_| AtomicU64::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        }
    }

    fn clear(&self) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }

    fn record(&self, owner: u64, method: u64, value: u32) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(call) = self.calls.get(index) {
            call.store((owner << 48) | (method << 40) | u64::from(value), Ordering::Relaxed);
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }

    fn observed(&self) -> Vec<(u64, u64, u32)> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.calls[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|call| {
                let value = call.load(Ordering::Relaxed);
                (value >> 48, (value >> 40) & 255, value as u32)
            })
            .collect()
    }
}

struct Facts {
    rate: AtomicU32,
    valid: AtomicBool,
    local: AtomicBool,
    latency: AtomicU32,
    requested: AtomicBool,
    recovery: AtomicBool,
    local_after_stop: AtomicBool,
}

impl Facts {
    fn new() -> Self {
        Self {
            rate: AtomicU32::new(0),
            valid: AtomicBool::new(true),
            local: AtomicBool::new(false),
            latency: AtomicU32::new(0),
            requested: AtomicBool::new(false),
            recovery: AtomicBool::new(false),
            local_after_stop: AtomicBool::new(false),
        }
    }
}

struct Probe {
    owner: u64,
    facts: Arc<Facts>,
    trace: Arc<Trace>,
}

// Both public device traits must expose the same scripted boundary facts.
macro_rules! observation_methods {
    () => {
        fn activation_sample_rate(&self) -> Option<u32> {
            let rate = self.facts.rate.load(Ordering::Acquire);
            self.trace.record(self.owner, RATE, rate);
            (rate != 0).then_some(rate)
        }
        fn processing_configuration_valid(&self) -> bool {
            let valid = self.facts.valid.load(Ordering::Acquire);
            self.trace.record(self.owner, VALID, u32::from(valid));
            valid
        }
        fn processing_failure_is_local(&self) -> bool {
            let local = self.facts.local.load(Ordering::Acquire);
            self.trace.record(self.owner, LOCAL, u32::from(local));
            local
        }
        fn latency_samples(&self) -> u32 {
            let latency = self.facts.latency.load(Ordering::Acquire);
            self.trace.record(self.owner, LATENCY, latency);
            latency
        }
        fn reconfiguration_requested(&self) -> bool {
            let requested = self.facts.requested.load(Ordering::Acquire);
            self.trace.record(self.owner, REQUEST, u32::from(requested));
            requested
        }
        fn processing_recovery_requested(&self) -> bool {
            let recovery = self.facts.recovery.load(Ordering::Acquire);
            self.trace.record(self.owner, RECOVERY, u32::from(recovery));
            recovery
        }
        fn stop_for_reconfiguration(&mut self) {
            let local = self.facts.local_after_stop.load(Ordering::Acquire);
            self.trace.record(self.owner, STOP, u32::from(local));
            self.facts.local.store(local, Ordering::Release);
            self.facts.recovery.store(local, Ordering::Release);
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
        fn reset(&mut self) {}
    };
}

impl vibez_dsp::effect::AudioEffect for Probe {
    observation_methods!();
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
}

impl vibez_instruments::Instrument for Probe {
    observation_methods!();
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, buffer: &mut [f32], _: usize) {
        buffer.fill(0.0);
    }
}

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    tracks: [TrackId; 2],
    effects: [EffectId; 3],
    model: Vec<RoutingChannel>,
    facts: [Arc<Facts>; 4],
    trace: Arc<Trace>,
}

fn fixture() -> Fixture {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let tracks = [TrackId::new(), TrackId::new()];
    let effects = [EffectId::new(), EffectId::new(), EffectId::new()];
    let facts: [Arc<Facts>; 4] = std::array::from_fn(|_| Arc::new(Facts::new()));
    let trace = Arc::new(Trace::new());
    let probe = |owner: usize| Probe {
        owner: owner as u64,
        facts: Arc::clone(&facts[owner - 1]),
        trace: Arc::clone(&trace),
    };
    let mut first = EngineTrack::new(tracks[0]);
    first.instrument = Some(Box::new(probe(1)));
    first.playback_source.clips.push(constant_clip(4096, 0.25));
    for (owner, id) in [(2, effects[0]), (3, effects[1])] {
        first.effects.push(EffectSlot { id, effect: Box::new(probe(owner)), bypass: false });
    }
    let mut second = EngineTrack::new(tracks[1]);
    second.effects.push(EffectSlot { id: effects[2], effect: Box::new(probe(4)), bypass: false });
    let mut first_channel = routing_channel(tracks[0]);
    first_channel.effects.extend(effects[..2].iter().copied().map(routing_effect));
    let mut second_channel = routing_channel(tracks[1]);
    second_channel.effects.push(routing_effect(effects[2]));
    let model = vec![first_channel, second_channel, routing_channel(TrackId::MASTER)];
    commands.push(EngineCommand::AddTrack(Box::new(first))).unwrap();
    commands.push(EngineCommand::AddTrack(Box::new(second))).unwrap();
    commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&model, 64).unwrap())).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    commands.push(EngineCommand::StartPerformanceCapture).unwrap();
    engine.drain_commands();
    assert!(engine.capture_active);
    assert!(engine.compensation_plan.applied_plan_is_valid());
    while events.pop().is_ok() {}
    trace.clear();
    Fixture { engine, commands, events, tracks, effects, model, facts, trace }
}

#[derive(Debug, Clone, Copy)]
enum ValidationCase {
    Healthy,
    InstrumentRate,
    EffectRate,
    Configuration,
    LocalOnly,
    LocalThenInvalid,
    Latency,
    RateBeforeLocal,
    MissingEffect,
    MissingTrack,
    MissingInstrument,
    LocalAway,
}

fn valid_calls(owner: u64) -> [(u64, u64, u32); 4] {
    [(owner, RATE, 0), (owner, VALID, 1), (owner, LOCAL, 0), (owner, LATENCY, 0)]
}

#[test]
fn validation_preserves_order_first_invalid_reason_and_accumulated_local_failure() {
    for case in [ValidationCase::Healthy, ValidationCase::InstrumentRate, ValidationCase::EffectRate,
        ValidationCase::Configuration, ValidationCase::LocalOnly, ValidationCase::LocalThenInvalid,
        ValidationCase::Latency, ValidationCase::RateBeforeLocal, ValidationCase::MissingEffect,
        ValidationCase::MissingTrack, ValidationCase::MissingInstrument, ValidationCase::LocalAway] {
        let mut fixture = fixture();
        let mut held_device = None;
        let mut expected = Vec::new();
        let mut failure = None;
        let mut local = false;
        let rate_reason = "Device was activated at a different sample rate; recreate the device or reopen the project";
        let config_reason = "Device processing configuration is unavailable or failed";
        let latency_reason = "Device latency no longer matches the applied compensation plan";
        match case {
            ValidationCase::Healthy => expected.extend((1..=4).flat_map(valid_calls)),
            ValidationCase::InstrumentRate => {
                fixture.facts[0].rate.store(48_000, Ordering::Release);
                expected.push((1, RATE, 48_000));
                failure = Some((fixture.tracks[0], None, rate_reason));
            }
            ValidationCase::LocalAway => {
                local = true;
                fixture.facts[1].requested.store(true, Ordering::Release);
                fixture.facts[1].local_after_stop.store(true, Ordering::Release);
                let owner = reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect);
                assert_eq!(crate::retirement::tests::allocations(|| fixture.engine.begin_device_reconfiguration()), (0, 0));
                while let Ok(event) = fixture.events.pop() {
                    if let EngineEvent::DeviceReconfiguration(device) = event {
                        assert!(held_device.is_none());
                        assert_eq!(device.owner_identity(), owner);
                        held_device = Some(device);
                    } else {
                        panic!("unexpected local transfer event: {event:?}");
                    }
                }
                assert!(held_device.is_some());
                assert!(fixture.engine.device_handoff.pending().unwrap().is_local_recovery());
                fixture.trace.clear();
                expected.extend([1, 3, 4].into_iter().flat_map(valid_calls));
            }
            ValidationCase::MissingTrack | ValidationCase::MissingInstrument | ValidationCase::MissingEffect => {
                fixture.commands.push(match case {
                    ValidationCase::MissingTrack => EngineCommand::RemoveTrack(fixture.tracks[0]),
                    ValidationCase::MissingInstrument => EngineCommand::RemoveTrackInstrument(fixture.tracks[0]),
                    _ => EngineCommand::RemoveEffect(fixture.tracks[0], fixture.effects[0]),
                }).unwrap();
                fixture.commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&fixture.model, 64).unwrap())).unwrap();
                fixture.engine.drain_commands();
                while fixture.events.pop().is_ok() {}
                fixture.trace.clear();
                match case {
                    ValidationCase::MissingTrack => expected.extend(valid_calls(4)),
                    ValidationCase::MissingInstrument => expected.extend((2..=4).flat_map(valid_calls)),
                    _ => {
                        expected.extend(valid_calls(1));
                        failure = Some((fixture.tracks[0], Some(fixture.effects[0]), config_reason));
                    }
                }
            }
            _ => {
                expected.extend(valid_calls(1));
                match case {
                    ValidationCase::EffectRate | ValidationCase::RateBeforeLocal => {
                        fixture.facts[1].rate.store(48_000, Ordering::Release);
                        if matches!(case, ValidationCase::RateBeforeLocal) {
                            fixture.facts[1].local.store(true, Ordering::Release);
                            fixture.facts[1].valid.store(false, Ordering::Release);
                        }
                        expected.push((2, RATE, 48_000));
                        failure = Some((fixture.tracks[0], Some(fixture.effects[0]), rate_reason));
                    }
                    ValidationCase::Configuration => {
                        fixture.facts[1].valid.store(false, Ordering::Release);
                        expected.extend([(2, RATE, 0), (2, VALID, 0), (2, LOCAL, 0)]);
                        failure = Some((fixture.tracks[0], Some(fixture.effects[0]), config_reason));
                    }
                    ValidationCase::Latency => {
                        fixture.facts[1].latency.store(1, Ordering::Release);
                        expected.extend([(2, RATE, 0), (2, VALID, 1), (2, LOCAL, 0), (2, LATENCY, 1)]);
                        failure = Some((fixture.tracks[0], Some(fixture.effects[0]), latency_reason));
                    }
                    ValidationCase::LocalOnly | ValidationCase::LocalThenInvalid => {
                        local = true;
                        fixture.facts[1].valid.store(false, Ordering::Release);
                        fixture.facts[1].local.store(true, Ordering::Release);
                        fixture.facts[1].latency.store(521, Ordering::Release);
                        expected.extend([(2, RATE, 0), (2, VALID, 0), (2, LOCAL, 1)]);
                        if matches!(case, ValidationCase::LocalThenInvalid) {
                            fixture.facts[2].valid.store(false, Ordering::Release);
                            fixture.facts[3].rate.store(96_000, Ordering::Release);
                            expected.extend([(3, RATE, 0), (3, VALID, 0), (3, LOCAL, 0)]);
                            failure = Some((fixture.tracks[0], Some(fixture.effects[1]), config_reason));
                        } else {
                            expected.extend((3..=4).flat_map(valid_calls));
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        assert_eq!(crate::retirement::tests::allocations(|| fixture.engine.validate_compensation()), (0, 0));
        let calls = fixture.trace.observed();
        assert_eq!(calls, expected, "{case:?}");
        let mut order = Vec::new();
        let mut failures = Vec::new();
        let mut capture_stops = Vec::new();
        while let Ok(event) = fixture.events.pop() {
            match event {
                EngineEvent::PerformanceCaptureStopped { effective_at_samples } => {
                    order.push("capture-stop");
                    capture_stops.push(effective_at_samples);
                }
                EngineEvent::CompensationInvalid { track_id, effect_id, reason } => {
                    order.push("invalid");
                    failures.push((track_id, effect_id, reason));
                }
                EngineEvent::PlaybackStopped => order.push("playback-stop"),
                event => panic!("unexpected validation event: {event:?}"),
            }
        }
        assert_eq!(failures, failure.into_iter().collect::<Vec<_>>(), "{case:?}");
        let mut expected_order = Vec::new();
        if local {
            expected_order.push("capture-stop");
        }
        if failure.is_some() {
            expected_order.extend(["capture-stop", "invalid", "playback-stop"]);
        }
        assert_eq!(order, expected_order, "{case:?}");
        assert!(capture_stops.iter().all(|position| *position == 0));
        assert_eq!(fixture.engine.compensation_plan.applied_plan_is_valid(), failure.is_none());
        assert_eq!(fixture.engine.transport.is_playing(), failure.is_none());
        assert_eq!(fixture.engine.capture_active, !local && failure.is_none());
        assert_eq!(fixture.engine.device_failure_capture_closed, local);
        assert_eq!(fixture.engine.transport.position(), 0);
        assert_eq!(fixture.engine.output_position, 0);
        assert_eq!(held_device.is_some(), matches!(case, ValidationCase::LocalAway));
        eprintln!("device-observation-b-validation: case={case:?}; calls={calls:?}; order={order:?}; capture_stops={capture_stops:?}; failure_owner={}; failure_reason={:?}; valid={}; playing={}; capture={}; local_episode={local}",
            failure.map_or(0, |(_, effect, _)| effect.map_or(1, |id| if id == fixture.effects[0] { 2 } else { 3 })),
            failure.map(|(_, _, reason)| reason), fixture.engine.compensation_plan.applied_plan_is_valid(),
            fixture.engine.transport.is_playing(), fixture.engine.capture_active);
    }
}

