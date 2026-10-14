//! Capture admission observes device changes at each actual command in one drain.

use super::*;
use graph_test_support::{constant_clip, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;
use vibez_core::midi::InstrumentKind;
use vibez_core::routing::RoutingChannel;

struct Calls {
    entries: [AtomicU32; 128],
    len: AtomicUsize,
    overflow: AtomicBool,
}

impl Calls {
    fn new() -> Self {
        Self { entries: std::array::from_fn(|_| AtomicU32::new(0)), len: AtomicUsize::new(0), overflow: AtomicBool::new(false) }
    }
    fn clear(&self) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }
    fn record(&self, owner: u32, method: u32, value: bool) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(entry) = self.entries.get(index) {
            entry.store(owner * 100 + method * 10 + u32::from(value), Ordering::Relaxed);
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }
    fn observed(&self) -> Vec<u32> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.entries[..self.len.load(Ordering::Relaxed)].iter().map(|entry| entry.load(Ordering::Relaxed)).collect()
    }
}

struct Control {
    local: AtomicBool,
    offer_handoff: AtomicBool,
    stopped: AtomicBool,
    drops: AtomicUsize,
}

impl Control {
    fn new() -> Self {
        Self { local: AtomicBool::new(false), offer_handoff: AtomicBool::new(false), stopped: AtomicBool::new(false), drops: AtomicUsize::new(0) }
    }
}

struct Probe {
    owner: u32,
    control: Arc<Control>,
    calls: Arc<Calls>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.control.drops.fetch_add(1, Ordering::Relaxed);
    }
}

macro_rules! capture_observations {
    () => {
        fn processing_failure_is_local(&self) -> bool {
            let local = self.control.local.load(Ordering::Acquire);
            self.calls.record(self.owner, 1, local);
            local
        }
        fn reconfiguration_requested(&self) -> bool {
            self.control.offer_handoff.load(Ordering::Acquire) && self.control.local.load(Ordering::Acquire)
        }
        fn processing_recovery_requested(&self) -> bool {
            self.control.offer_handoff.load(Ordering::Acquire) && self.control.local.load(Ordering::Acquire)
        }
        fn stop_for_reconfiguration(&mut self) {
            self.control.stopped.store(true, Ordering::Release);
            self.calls.record(self.owner, 5, true);
        }
        fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
            assert!(self.control.stopped.load(Ordering::Acquire));
            self.control.local.store(false, Ordering::Release);
            self.control.stopped.store(false, Ordering::Release);
            self.calls.record(self.owner, 4, false);
            Ok(())
        }
        fn param_descriptors(&self) -> &'static [ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, index: usize, value: f32) -> bool {
            if index != 0 {
                return false;
            }
            let local = value != 0.0;
            self.control.local.store(local, Ordering::Release);
            self.calls.record(self.owner, 2, local);
            true
        }
        fn get_param(&self, _: usize) -> f32 {
            f32::from(u8::from(self.control.local.load(Ordering::Acquire)))
        }
        fn reset(&mut self) {
            // A reset-sensitive native device models command-time changes;
            // this is not a claim that hosted processing failure clears on Play.
            self.control.local.store(false, Ordering::Release);
            self.calls.record(self.owner, 3, false);
        }
    };
}

impl vibez_dsp::effect::AudioEffect for Probe {
    capture_observations!();
    fn effect_type(&self) -> EffectType { EffectType::Gain }
    fn process(&mut self, _: &mut [f32], _: usize) {}
}

impl vibez_instruments::Instrument for Probe {
    capture_observations!();
    fn instrument_kind(&self) -> InstrumentKind { InstrumentKind::SubtractiveSynth }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, buffer: &mut [f32], _: usize) { buffer.fill(0.0); }
}

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    effect: EffectId,
    model: Vec<RoutingChannel>,
    controls: [Arc<Control>; 2],
    calls: Arc<Calls>,
}

fn fixture(playing: bool) -> Fixture {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let controls = [Arc::new(Control::new()), Arc::new(Control::new())];
    let calls = Arc::new(Calls::new());
    let mut source = EngineTrack::new(track);
    source.instrument = Some(Box::new(Probe { owner: 1, control: Arc::clone(&controls[0]), calls: Arc::clone(&calls) }));
    source.effects.push(EffectSlot { id: effect, effect: Box::new(Probe { owner: 2, control: Arc::clone(&controls[1]), calls: Arc::clone(&calls) }), bypass: false });
    source.playback_source.clips.push(constant_clip(4096, 0.25));
    let mut channel = routing_channel(track);
    channel.effects.push(routing_effect(effect));
    let model = vec![channel, routing_channel(TrackId::MASTER)];
    commands.push(EngineCommand::AddTrack(Box::new(source))).unwrap();
    commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&model, 64).unwrap())).unwrap();
    if playing { commands.push(EngineCommand::Play).unwrap(); }
    engine.drain_commands();
    while events.pop().is_ok() {}
    calls.clear();
    Fixture { engine, commands, events, track, effect, model, controls, calls }
}

fn set_local(fixture: &Fixture, instrument: bool, local: bool) -> EngineCommand {
    if instrument {
        EngineCommand::SetInstrumentParam { track_id: fixture.track, param_index: 0, value: f32::from(u8::from(local)) }
    } else {
        EngineCommand::SetEffectParam { track_id: fixture.track, effect_id: fixture.effect, param_index: 0, value: f32::from(u8::from(local)) }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Events {
    order: Vec<&'static str>,
    reasons: Vec<&'static str>,
    positions: Vec<u64>,
}

fn collect(fixture: &mut Fixture) -> Events {
    let mut result = Events { order: Vec::new(), reasons: Vec::new(), positions: Vec::new() };
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStarted { offsets, effective_at_samples, section_id, section_position_samples } => {
                assert!(Arc::ptr_eq(&offsets, &fixture.engine.routing.capture_offsets));
                assert_eq!((section_id, section_position_samples), (None, None));
                result.order.push("start"); result.positions.push(effective_at_samples);
            }
            EngineEvent::ClipCaptureSource { track_id, position, effective_at_samples } => {
                assert_eq!(track_id, fixture.track); assert_eq!(position, 0);
                result.order.push("source"); result.positions.push(effective_at_samples);
            }
            EngineEvent::PerformanceCaptureStopped { effective_at_samples } => {
                result.order.push("stop"); result.positions.push(effective_at_samples);
            }
            EngineEvent::CompensationInvalid { track_id, effect_id, reason } => {
                assert_eq!((track_id, effect_id), (TrackId::MASTER, None));
                result.order.push("refused"); result.reasons.push(reason);
            }
            EngineEvent::DisposeEffect(_) => result.order.push("retire-effect"),
            EngineEvent::DisposeInstrument(_) => result.order.push("retire-instrument"),
            EngineEvent::RoutingRetired(_) => result.order.push("routing-retired"),
            EngineEvent::PlaybackStarted => result.order.push("play"),
            event => panic!("unexpected Capture admission event: {event:?}"),
        }
    }
    assert!(result.positions.iter().all(|position| *position == 0));
    result
}

fn drain(fixture: &mut Fixture) {
    assert_eq!(crate::retirement::tests::allocations(|| fixture.engine.drain_commands()), (0, 0));
    assert!(fixture.engine.cmd_rx.peek().is_err());
    assert_eq!(fixture.engine.transport.position(), 0);
    assert_eq!(fixture.engine.performance_position, 0);
    assert_eq!(fixture.engine.output_position, 0);
}

fn report(case: &str, instrument: bool, before: bool, calls: &[u32], events: &Events, fixture: &Fixture) {
    eprintln!("device-observation-c: case={case}; instrument={instrument}; before={before}; calls={calls:?}; order={:?}; reasons={:?}; positions={:?}; capture={}; playing={}; valid={}; awaiting={}; pending={}",
        events.order, events.reasons, events.positions, fixture.engine.capture_active, fixture.engine.transport.is_playing(),
        fixture.engine.compensation_plan.applied_plan_is_valid(), fixture.engine.compensation_plan.awaiting_structural_plan(),
        fixture.engine.device_handoff.pending().is_some());
}

const LOCAL_REASON: &str = "Capture requires recovery or reload of the failed device";

#[test]
fn same_drain_capture_starts_observe_effect_and_instrument_manual_edits() {
    for instrument in [false, true] {
        for before in [false, true] {
            let mut fixture = fixture(true);
            fixture.controls[usize::from(!instrument)].local.store(before, Ordering::Release);
            fixture.commands.push(EngineCommand::StartPerformanceCapture).unwrap();
            fixture.commands.push(set_local(&fixture, instrument, !before)).unwrap();
            fixture.commands.push(EngineCommand::StartPerformanceCapture).unwrap();
            drain(&mut fixture);
            let calls = fixture.calls.observed();
            let first = if instrument && before { vec![111] } else { vec![110, if before { 211 } else { 210 }] };
            let mut expected = first;
            expected.push(if instrument { 120 + u32::from(!before) } else { 220 + u32::from(!before) });
            expected.extend(if instrument && !before { vec![111] } else { vec![110, if before || instrument { 210 } else { 211 }] });
            assert_eq!(calls, expected);
            let events = collect(&mut fixture);
            assert_eq!(events.order, if before { vec!["stop", "refused", "start", "source"] } else { vec!["start", "source", "stop", "refused"] });
            assert_eq!(events.reasons, [LOCAL_REASON]);
            assert_eq!(fixture.engine.capture_active, before);
            assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
            report("manual-edit", instrument, before, &calls, &events, &fixture);
        }
    }
}

#[test]
fn a_play_reset_between_capture_commands_is_observed_in_the_same_drain() {
    let mut fixture = fixture(false);
    fixture.controls[1].local.store(true, Ordering::Release);
    fixture.commands.push(EngineCommand::StartPerformanceCapture).unwrap();
    fixture.commands.push(EngineCommand::Play).unwrap();
    fixture.commands.push(EngineCommand::StartPerformanceCapture).unwrap();
    drain(&mut fixture);
    let calls = fixture.calls.observed();
    assert_eq!(calls, [110, 211, 230, 110, 210]);
    let events = collect(&mut fixture);
    assert_eq!(events.order, ["stop", "refused", "play", "start", "source"]);
    assert_eq!(events.reasons, [LOCAL_REASON]);
    assert!(fixture.engine.capture_active);
    assert!(fixture.engine.transport.is_playing());
    report("play-reset", false, true, &calls, &events, &fixture);
}
