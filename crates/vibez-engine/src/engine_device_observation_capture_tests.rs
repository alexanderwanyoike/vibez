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
        Self {
            entries: std::array::from_fn(|_| AtomicU32::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        }
    }
    fn clear(&self) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }
    fn record(&self, owner: u32, method: u32, value: bool) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(entry) = self.entries.get(index) {
            entry.store(
                owner * 100 + method * 10 + u32::from(value),
                Ordering::Relaxed,
            );
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }
    fn observed(&self) -> Vec<u32> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.entries[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|entry| entry.load(Ordering::Relaxed))
            .collect()
    }
    fn assert_local_answer(&self, owner: u32, local: bool) {
        assert!(
            self.observed()
                .contains(&(owner * 100 + 10 + u32::from(local))),
            "device {owner} must expose local failure = {local}"
        );
    }

    fn assert_local_transition(&self, owner: u32, before: bool, after: bool, reset: bool) {
        let calls = self.observed();
        let change = owner * 100 + if reset { 30 } else { 20 + u32::from(after) };
        let boundary = calls.iter().position(|call| *call == change).unwrap();
        assert!(
            calls[..boundary].contains(&(owner * 100 + 10 + u32::from(before))),
            "the first Start must observe the previous local-failure answer"
        );
        assert!(
            calls[boundary + 1..].contains(&(owner * 100 + 10 + u32::from(after))),
            "the later Start must observe the actual intervening edit or reset"
        );
    }

    fn assert_replacement_observed(&self, old_owner: u32, new_owner: u32) {
        let calls = self.observed();
        let failed = calls
            .iter()
            .position(|call| *call == old_owner * 100 + 11)
            .unwrap();
        assert!(
            calls[failed + 1..].contains(&(new_owner * 100 + 10)),
            "the later Start must sample a current healthy owner after the failed one"
        );
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
        Self {
            local: AtomicBool::new(false),
            offer_handoff: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            drops: AtomicUsize::new(0),
        }
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
            self.control.offer_handoff.load(Ordering::Acquire)
                && self.control.local.load(Ordering::Acquire)
        }
        fn processing_recovery_requested(&self) -> bool {
            self.control.offer_handoff.load(Ordering::Acquire)
                && self.control.local.load(Ordering::Acquire)
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
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
}

impl vibez_instruments::Instrument for Probe {
    capture_observations!();
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
    source.instrument = Some(Box::new(Probe {
        owner: 1,
        control: Arc::clone(&controls[0]),
        calls: Arc::clone(&calls),
    }));
    source.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(Probe {
            owner: 2,
            control: Arc::clone(&controls[1]),
            calls: Arc::clone(&calls),
        }),
        bypass: false,
    });
    source.playback_source.clips.push(constant_clip(4096, 0.25));
    let mut channel = routing_channel(track);
    channel.effects.push(routing_effect(effect));
    let model = vec![channel, routing_channel(TrackId::MASTER)];
    commands
        .push(EngineCommand::AddTrack(Box::new(source)))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 64).unwrap(),
        ))
        .unwrap();
    if playing {
        commands.push(EngineCommand::Play).unwrap();
    }
    engine.drain_commands();
    while events.pop().is_ok() {}
    calls.clear();
    Fixture {
        engine,
        commands,
        events,
        track,
        effect,
        model,
        controls,
        calls,
    }
}

fn set_local(fixture: &Fixture, instrument: bool, local: bool) -> EngineCommand {
    if instrument {
        EngineCommand::SetInstrumentParam {
            track_id: fixture.track,
            param_index: 0,
            value: f32::from(u8::from(local)),
        }
    } else {
        EngineCommand::SetEffectParam {
            track_id: fixture.track,
            effect_id: fixture.effect,
            param_index: 0,
            value: f32::from(u8::from(local)),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Events {
    order: Vec<&'static str>,
    reasons: Vec<&'static str>,
    positions: Vec<u64>,
}

fn collect(fixture: &mut Fixture) -> Events {
    let mut result = Events {
        order: Vec::new(),
        reasons: Vec::new(),
        positions: Vec::new(),
    };
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStarted {
                offsets,
                effective_at_samples,
                section_id,
                section_position_samples,
            } => {
                assert!(Arc::ptr_eq(
                    &offsets,
                    &fixture.engine.routing.capture_offsets
                ));
                assert_eq!((section_id, section_position_samples), (None, None));
                result.order.push("start");
                result.positions.push(effective_at_samples);
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                assert_eq!(track_id, fixture.track);
                assert_eq!(position, 0);
                result.order.push("source");
                result.positions.push(effective_at_samples);
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                result.order.push("stop");
                result.positions.push(effective_at_samples);
            }
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!((track_id, effect_id), (TrackId::MASTER, None));
                result.order.push("refused");
                result.reasons.push(reason);
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
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
    assert_eq!(fixture.engine.transport.position(), 0);
    assert_eq!(fixture.engine.performance_position, 0);
    assert_eq!(fixture.engine.output_position, 0);
}

const LOCAL_REASON: &str = "Capture requires recovery or reload of the failed device";

#[test]
fn same_drain_capture_starts_observe_effect_and_instrument_manual_edits() {
    for instrument in [false, true] {
        for before in [false, true] {
            let mut fixture = fixture(true);
            fixture.controls[usize::from(!instrument)]
                .local
                .store(before, Ordering::Release);
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            fixture
                .commands
                .push(set_local(&fixture, instrument, !before))
                .unwrap();
            fixture
                .commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
            drain(&mut fixture);
            fixture.calls.assert_local_transition(
                if instrument { 1 } else { 2 },
                before,
                !before,
                false,
            );
            let events = collect(&mut fixture);
            assert_eq!(
                events.order,
                if before {
                    vec!["stop", "refused", "start", "source"]
                } else {
                    vec!["start", "source", "stop", "refused"]
                }
            );
            assert_eq!(events.reasons, [LOCAL_REASON]);
            assert_eq!(fixture.engine.capture_active, before);
            assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
        }
    }
}

#[test]
fn a_play_reset_between_capture_commands_is_observed_in_the_same_drain() {
    let mut fixture = fixture(false);
    fixture.controls[1].local.store(true, Ordering::Release);
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    fixture.commands.push(EngineCommand::Play).unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    fixture.calls.assert_local_transition(2, true, false, true);
    let events = collect(&mut fixture);
    assert_eq!(events.order, ["stop", "refused", "play", "start", "source"]);
    assert_eq!(events.reasons, [LOCAL_REASON]);
    assert!(fixture.engine.capture_active);
    assert!(fixture.engine.transport.is_playing());
}

#[test]
fn same_drain_capture_admission_observes_removed_and_replaced_device_owners() {
    for instrument in [false, true] {
        let mut fixture = fixture(true);
        let target = usize::from(!instrument);
        fixture.controls[target]
            .local
            .store(true, Ordering::Release);
        let replacement = Arc::new(Control::new());
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        fixture
            .commands
            .push(if instrument {
                EngineCommand::SetInstrument {
                    track_id: fixture.track,
                    instrument: Box::new(Probe {
                        owner: 3,
                        control: Arc::clone(&replacement),
                        calls: Arc::clone(&fixture.calls),
                    }),
                }
            } else {
                EngineCommand::RemoveEffect(fixture.track, fixture.effect)
            })
            .unwrap();
        fixture
            .commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        drain(&mut fixture);
        assert_eq!(
            fixture.controls[target].drops.load(Ordering::Relaxed),
            0,
            "the callback returns the previous owner"
        );
        fixture.calls.assert_replacement_observed(
            if instrument { 1 } else { 2 },
            if instrument { 3 } else { 1 },
        );
        let events = collect(&mut fixture);
        assert_eq!(
            events.order,
            [
                "stop",
                "refused",
                if instrument {
                    "retire-instrument"
                } else {
                    "retire-effect"
                },
                "start",
                "source"
            ]
        );
        assert_eq!(events.reasons, [LOCAL_REASON]);
        assert_eq!(fixture.controls[target].drops.load(Ordering::Relaxed), 1);
        assert_eq!(replacement.drops.load(Ordering::Relaxed), 0);
        assert!(fixture.engine.capture_active);
        assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
        assert_eq!(
            fixture.engine.compensation_plan.awaiting_structural_plan(),
            !instrument
        );
    }
}

#[test]
fn current_local_resume_changes_capture_admission_at_its_original_same_drain_boundary() {
    let mut fixture = fixture(true);
    fixture.controls[1].local.store(true, Ordering::Release);
    fixture.controls[1]
        .offer_handoff
        .store(true, Ordering::Release);
    let original =
        reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.begin_device_reconfiguration()),
        (0, 0)
    );
    let mut owner = match fixture.events.pop().unwrap() {
        EngineEvent::DeviceReconfiguration(device) => device,
        event => panic!("expected actual local handoff, got{event:?}"),
    };
    assert!(fixture.events.is_empty());
    assert!(fixture
        .engine
        .device_handoff
        .pending()
        .unwrap()
        .is_local_recovery());
    assert_eq!(owner.owner_identity(), original);
    owner.reconfigure_on_main_thread().unwrap();
    fixture.calls.clear();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    // This no-op marks the end of token-owned refusal without freezing reads after Resume.
    fixture
        .commands
        .push(set_local(&fixture, true, false))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::ResumeDeviceReconfiguration {
            device: owner,
            routing: crate::routing::PreparedRouting::prepare(&fixture.model, 64).unwrap(),
        })
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    let calls = fixture.calls.observed();
    let before_resume = calls.iter().position(|call| *call == 120).unwrap();
    assert!(
        !calls[..before_resume]
            .iter()
            .any(|call| call % 100 / 10 == 1),
        "a token-owned refusal must not query resident devices"
    );
    fixture.calls.assert_local_answer(1, false);
    fixture.calls.assert_local_answer(2, false);
    let events = collect(&mut fixture);
    assert_eq!(
        events.order,
        ["stop", "refused", "routing-retired", "start", "source"]
    );
    assert_eq!(events.reasons, [LOCAL_REASON]);
    assert!(fixture.engine.capture_active);
    assert!(fixture.engine.device_handoff.pending().is_none());
    assert_eq!(fixture.engine.device_handoff.last_issued_identity(), 1);
    assert_eq!(
        reconfiguration::effect_owner_identity(&*fixture.engine.tracks[0].effects[0].effect),
        original
    );
}

#[test]
fn local_failure_keeps_refusal_priority_then_same_drain_clear_reveals_invalid_plan() {
    let mut fixture = fixture(true);
    fixture
        .commands
        .push(EngineCommand::insert_built_in_effect(
            fixture.track,
            EffectId::new(),
            EffectType::Gain,
            DEFAULT_SAMPLE_RATE,
        ))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::RejectRoutingUpdate {
            reason: "Capture admission baseline".into(),
        })
        .unwrap();
    fixture.engine.drain_commands();
    while fixture.events.pop().is_ok() {}
    fixture.controls[0].local.store(true, Ordering::Release);
    fixture.calls.clear();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    fixture
        .commands
        .push(set_local(&fixture, true, false))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    drain(&mut fixture);
    fixture.calls.assert_local_transition(1, true, false, false);
    let events = collect(&mut fixture);
    assert_eq!(events.order, ["stop", "refused", "stop", "refused"]);
    assert_eq!(
        events.reasons,
        [
            LOCAL_REASON,
            "Capture requires a valid applied audio configuration"
        ]
    );
    assert!(!fixture.engine.capture_active);
    assert!(!fixture.engine.compensation_plan.applied_plan_is_valid());
    assert!(fixture.engine.compensation_plan.awaiting_structural_plan());
}
